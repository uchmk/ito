//! Short sounds of the app's own, played the moment they are asked for: a tick
//! per keystroke, a click, a tone. One output stays open; [`Player::play`]
//! only adds a voice to a mixer and returns, and plays may overlap.
//!
//! Windows (`waveOut`) and macOS (AudioQueue) have it. Elsewhere
//! [`Player::new`] says no, so a caller that wants no sound there simply keeps
//! on without one; a Linux backend would pull in ALSA's C library, which the
//! other crates avoid. The output is opened once, ahead of the first sound,
//! and mixes silence while idle: make the player when the sound is on, drop
//! it when it is off.
//!
//! [`sound::play`](crate::sound::play), by contrast, plays one of the
//! system's own sounds on a thread of its own: right for a notification, too
//! slow for a keystroke.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

/// The rate the output runs at; every sample is brought to it when loaded.
pub const RATE: u32 = 44_100;

/// Voices playing at once; one more takes the oldest's place.
const MAX_VOICES: usize = 32;

/// A sound ready to play: mono, at [`RATE`]. Cheap to clone.
#[derive(Clone, Debug)]
pub struct Sample(Arc<[f32]>);

impl Sample {
    /// From mono samples in -1.0..=1.0 at `rate` Hz (linear resampling).
    pub fn from_pcm(samples: &[f32], rate: u32) -> Sample {
        if rate == 0 || rate == RATE {
            return Sample(samples.into());
        }
        let n = (samples.len() as u64 * RATE as u64 / rate as u64) as usize;
        let step = rate as f64 / RATE as f64;
        let at = |i: usize| samples.get(i).copied().unwrap_or(0.0);
        let out: Vec<f32> = (0..n)
            .map(|k| {
                let x = k as f64 * step;
                let (i, f) = (x as usize, (x - x.floor()) as f32);
                at(i) * (1.0 - f) + at(i + 1) * f
            })
            .collect();
        Sample(out.into())
    }

    /// From a WAV file's bytes: PCM of 8 or 16 bits (or 32-bit float), any
    /// channels (mixed down to one), any rate.
    pub fn from_wav(bytes: &[u8]) -> Result<Sample, String> {
        let bad = |why: &str| format!("not a WAV sound: {why}");
        if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
            return Err(bad("no RIFF/WAVE header"));
        }
        let le16 = |b: &[u8]| u16::from_le_bytes([b[0], b[1]]);
        let le32 = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        let (mut fmt, mut data) = (None, None);
        let mut at = 12;
        while at + 8 <= bytes.len() {
            let (id, len) = (&bytes[at..at + 4], le32(&bytes[at + 4..]) as usize);
            let body = &bytes[at + 8..(at + 8 + len).min(bytes.len())];
            match id {
                b"fmt " if body.len() >= 16 => fmt = Some((le16(body), le16(&body[2..]), le32(&body[4..]), le16(&body[14..]))),
                b"data" => data = Some(body),
                _ => {}
            }
            at += 8 + len + (len & 1);
        }
        let (tag, channels, rate, bits) = fmt.ok_or_else(|| bad("no fmt chunk"))?;
        let data = data.ok_or_else(|| bad("no data chunk"))?;
        let channels = channels.max(1) as usize;
        let one: fn(&[u8]) -> f32 = match (tag, bits) {
            (1, 8) => |b| (b[0] as f32 - 128.0) / 128.0,
            (1, 16) => |b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0,
            (3, 32) => |b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            _ => return Err(bad(&format!("format {tag} with {bits} bits"))),
        };
        let width = bits as usize / 8;
        let mono: Vec<f32> =
            data.chunks_exact(width * channels).map(|frame| frame.chunks_exact(width).map(one).sum::<f32>() / channels as f32).collect();
        Ok(Sample::from_pcm(&mono, rate))
    }

    /// How long it plays.
    pub fn duration(&self) -> std::time::Duration {
        std::time::Duration::from_secs_f64(self.0.len() as f64 / RATE as f64)
    }
}

struct Voice {
    sample: Arc<[f32]>,
    at: usize,
    gain: f32,
}

/// The sounds playing now, added together into the output's buffers.
#[derive(Default)]
struct Mixer {
    voices: Vec<Voice>,
}

impl Mixer {
    fn add(&mut self, sample: &Sample, gain: f32) {
        if sample.0.is_empty() || !gain.is_finite() || gain <= 0.0 {
            return;
        }
        if self.voices.len() >= MAX_VOICES {
            self.voices.remove(0);
        }
        self.voices.push(Voice { sample: sample.0.clone(), at: 0, gain });
    }

    /// Fill `out` (stereo, interleaved) with the next frames; silence when nothing plays.
    #[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
    fn fill(&mut self, out: &mut [i16]) {
        out.fill(0);
        for v in &mut self.voices {
            for frame in out.as_chunks_mut::<2>().0 {
                let Some(&s) = v.sample.get(v.at) else { break };
                v.at += 1;
                let mixed = (frame[0] as f32 + s * v.gain * 32767.0).clamp(-32768.0, 32767.0) as i16;
                frame[0] = mixed;
                frame[1] = mixed;
            }
        }
        self.voices.retain(|v| v.at < v.sample.len());
    }
}

fn lock(m: &Mutex<Mixer>) -> MutexGuard<'_, Mixer> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// One audio output, kept open.
pub struct Player {
    mixer: Arc<Mutex<Mixer>>,
    stop: Arc<AtomicBool>,
}

impl Player {
    /// Open the default output. `Err` when there is none (or on a system with
    /// no backend); the caller goes on without sound.
    pub fn new() -> Result<Player, String> {
        let mixer = Arc::new(Mutex::new(Mixer::default()));
        let stop = Arc::new(AtomicBool::new(false));
        os::start(mixer.clone(), stop.clone())?;
        Ok(Player { mixer, stop })
    }

    /// A sound from mono samples in -1.0..=1.0 at `rate` Hz.
    pub fn load_pcm(&self, samples: &[f32], rate: u32) -> Sample {
        Sample::from_pcm(samples, rate)
    }

    /// A sound from a WAV file's bytes.
    pub fn load_wav(&self, bytes: &[u8]) -> Result<Sample, String> {
        Sample::from_wav(bytes)
    }

    /// Start `sample` at `volume` (0.0..=1.0) and return at once. Sounds that
    /// are still playing go on; they add up.
    pub fn play(&self, sample: &Sample, volume: f32) {
        lock(&self.mixer).add(sample, volume);
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(windows)]
mod os {
    use super::{lock, Mixer, RATE};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc, Mutex};
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Media::Audio::{
        waveOutClose, waveOutOpen, waveOutPrepareHeader, waveOutReset, waveOutUnprepareHeader, waveOutWrite, CALLBACK_EVENT, HWAVEOUT,
        WAVEFORMATEX, WAVEHDR, WAVE_FORMAT_PCM, WAVE_MAPPER, WHDR_DONE,
    };
    use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

    /// Frames in one buffer (10 ms), and buffers queued ahead: about 30 ms.
    const FRAMES: usize = (RATE / 100) as usize;
    const BUFFERS: usize = 3;

    pub fn start(mixer: Arc<Mutex<Mixer>>, stop: Arc<AtomicBool>) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("sound-out".into())
            .spawn(move || {
                let _ = tx.send(run(&mixer, &stop, &tx));
            })
            .map_err(|e| e.to_string())?;
        // The thread answers once it has the output open (or has not), then keeps running.
        match rx.recv() {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("the sound thread ended".into()),
        }
    }

    fn run(mixer: &Mutex<Mixer>, stop: &AtomicBool, tx: &mpsc::Sender<Result<(), String>>) -> Result<(), String> {
        // SAFETY: plain Win32 calls; every handle and buffer below outlives the
        // output, which is reset and closed before this function returns.
        unsafe {
            let event = CreateEventW(None, false, false, None).map_err(|e| e.to_string())?;
            let result = play_loop(mixer, stop, event, tx);
            let _ = CloseHandle(event);
            result
        }
    }

    unsafe fn play_loop(
        mixer: &Mutex<Mixer>,
        stop: &AtomicBool,
        event: HANDLE,
        tx: &mpsc::Sender<Result<(), String>>,
    ) -> Result<(), String> {
        let format = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_PCM as u16,
            nChannels: 2,
            nSamplesPerSec: RATE,
            nAvgBytesPerSec: RATE * 4,
            nBlockAlign: 4,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        let mut out = HWAVEOUT::default();
        let r = waveOutOpen(Some(&mut out), WAVE_MAPPER, &format, Some(event.0 as usize), Some(0), CALLBACK_EVENT);
        if r != 0 {
            return Err(format!("no sound output (waveOutOpen {r})"));
        }
        let mut pcm = vec![[0i16; FRAMES * 2]; BUFFERS];
        let mut headers: Vec<WAVEHDR> = pcm
            .iter_mut()
            .map(|b| WAVEHDR {
                lpData: windows::core::PSTR(b.as_mut_ptr().cast()),
                dwBufferLength: (FRAMES * 4) as u32,
                ..Default::default()
            })
            .collect();
        let size = std::mem::size_of::<WAVEHDR>() as u32;
        for (h, b) in headers.iter_mut().zip(pcm.iter_mut()) {
            waveOutPrepareHeader(out, h, size);
            lock(mixer).fill(b);
            waveOutWrite(out, h, size);
        }
        let _ = tx.send(Ok(()));
        while !stop.load(Ordering::Relaxed) {
            WaitForSingleObject(event, 50);
            for (h, b) in headers.iter_mut().zip(pcm.iter_mut()) {
                if h.dwFlags & WHDR_DONE != 0 {
                    lock(mixer).fill(b);
                    waveOutWrite(out, h, size);
                }
            }
        }
        waveOutReset(out);
        for h in &mut headers {
            waveOutUnprepareHeader(out, h, size);
        }
        waveOutClose(out);
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod os {
    use super::{lock, Mixer, RATE};
    use std::ffi::c_void;
    use std::ptr;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    // AudioToolbox's queue API, by hand: a handful of calls, and no binding
    // crate to keep in step.
    #[repr(C)]
    struct Format {
        rate: f64,
        id: u32,
        flags: u32,
        bytes_per_packet: u32,
        frames_per_packet: u32,
        bytes_per_frame: u32,
        channels: u32,
        bits: u32,
        reserved: u32,
    }

    #[repr(C)]
    struct Buffer {
        capacity: u32,
        data: *mut c_void,
        size: u32,
        user: *mut c_void,
        packet_capacity: u32,
        packets: *const c_void,
        packet_count: u32,
    }

    type Queue = *mut c_void;
    type Callback = extern "C" fn(*mut c_void, Queue, *mut Buffer);

    #[link(name = "AudioToolbox", kind = "framework")]
    extern "C" {
        fn AudioQueueNewOutput(
            format: *const Format,
            callback: Callback,
            user: *mut c_void,
            run_loop: *mut c_void,
            mode: *const c_void,
            flags: u32,
            queue: *mut Queue,
        ) -> i32;
        fn AudioQueueAllocateBuffer(queue: Queue, bytes: u32, buffer: *mut *mut Buffer) -> i32;
        fn AudioQueueEnqueueBuffer(queue: Queue, buffer: *mut Buffer, packets: u32, descriptions: *const c_void) -> i32;
        fn AudioQueueStart(queue: Queue, start: *const c_void) -> i32;
        fn AudioQueueStop(queue: Queue, immediate: u8) -> i32;
        fn AudioQueueDispose(queue: Queue, immediate: u8) -> i32;
    }

    const FRAMES: u32 = RATE / 100;
    const BUFFERS: usize = 3;
    const LINEAR_PCM: u32 = u32::from_be_bytes(*b"lpcm");
    const SIGNED_PACKED: u32 = 4 | 8;

    struct Shared {
        mixer: Arc<Mutex<Mixer>>,
    }

    extern "C" fn refill(user: *mut c_void, queue: Queue, buffer: *mut Buffer) {
        // SAFETY: `user` is the `Shared` boxed in `start`, alive until the queue
        // is disposed (the callback does not run after that); the buffer is the
        // queue's own, of FRAMES stereo i16 frames.
        unsafe {
            let shared = &*(user as *const Shared);
            let b = &mut *buffer;
            let pcm = std::slice::from_raw_parts_mut(b.data as *mut i16, FRAMES as usize * 2);
            lock(&shared.mixer).fill(pcm);
            b.size = FRAMES * 4;
            AudioQueueEnqueueBuffer(queue, buffer, 0, ptr::null());
        }
    }

    pub fn start(mixer: Arc<Mutex<Mixer>>, stop: Arc<AtomicBool>) -> Result<(), String> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new().name("sound-out".into()).spawn(move || run(mixer, &stop, tx)).map_err(|e| e.to_string())?;
        match rx.recv() {
            Ok(r) => r,
            Err(_) => Err("the sound thread ended".into()),
        }
    }

    // The queue's callback runs on the system's own thread; this one only
    // owns the queue, and tears it down when the player is dropped.
    fn run(mixer: Arc<Mutex<Mixer>>, stop: &AtomicBool, tx: std::sync::mpsc::Sender<Result<(), String>>) {
        let shared = Box::new(Shared { mixer });
        let format = Format {
            rate: RATE as f64,
            id: LINEAR_PCM,
            flags: SIGNED_PACKED,
            bytes_per_packet: 4,
            frames_per_packet: 1,
            bytes_per_frame: 4,
            channels: 2,
            bits: 16,
            reserved: 0,
        };
        let mut queue: Queue = ptr::null_mut();
        // SAFETY: `format` and `queue` are live for the calls; `shared` outlives the queue.
        unsafe {
            let r =
                AudioQueueNewOutput(&format, refill, &*shared as *const Shared as *mut c_void, ptr::null_mut(), ptr::null(), 0, &mut queue);
            if r != 0 {
                let _ = tx.send(Err(format!("no sound output (AudioQueueNewOutput {r})")));
                return;
            }
            for _ in 0..BUFFERS {
                let mut buffer: *mut Buffer = ptr::null_mut();
                if AudioQueueAllocateBuffer(queue, FRAMES * 4, &mut buffer) != 0 {
                    let _ = tx.send(Err("no sound buffer".into()));
                    AudioQueueDispose(queue, 1);
                    return;
                }
                refill(&*shared as *const Shared as *mut c_void, queue, buffer);
            }
            let r = AudioQueueStart(queue, ptr::null());
            if r != 0 {
                let _ = tx.send(Err(format!("no sound output (AudioQueueStart {r})")));
                AudioQueueDispose(queue, 1);
                return;
            }
            let _ = tx.send(Ok(()));
            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            AudioQueueStop(queue, 1);
            AudioQueueDispose(queue, 1);
        }
        drop(shared);
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod os {
    use super::Mixer;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};

    pub fn start(_: Arc<Mutex<Mixer>>, _: Arc<AtomicBool>) -> Result<(), String> {
        Err("no sound player on this system".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(rate: u32, channels: u16, bits: u16, data: &[u8]) -> Vec<u8> {
        let mut v = b"RIFF".to_vec();
        v.extend((36 + data.len() as u32).to_le_bytes());
        v.extend(b"WAVEfmt ");
        v.extend(16u32.to_le_bytes());
        v.extend(1u16.to_le_bytes());
        v.extend(channels.to_le_bytes());
        v.extend(rate.to_le_bytes());
        v.extend((rate * channels as u32 * bits as u32 / 8).to_le_bytes());
        v.extend((channels * bits / 8).to_le_bytes());
        v.extend(bits.to_le_bytes());
        v.extend(b"data");
        v.extend((data.len() as u32).to_le_bytes());
        v.extend(data);
        v
    }

    #[test]
    fn a_rate_is_brought_to_the_output_rate() {
        let s = Sample::from_pcm(&vec![0.5; 22_050], 22_050);
        assert_eq!(s.0.len(), 44_100);
        assert!(s.0.iter().take(44_000).all(|&x| (x - 0.5).abs() < 1e-6));
        assert_eq!(Sample::from_pcm(&[0.1, 0.2], RATE).0.len(), 2);
    }

    #[test]
    fn a_wav_is_read_and_mixed_to_mono() {
        // Two stereo frames, 16 bits: (16384, -16384) and (32767, 32767).
        let mut data = Vec::new();
        for s in [16384i16, -16384, 32767, 32767] {
            data.extend(s.to_le_bytes());
        }
        let s = Sample::from_wav(&wav(RATE, 2, 16, &data)).unwrap();
        assert_eq!(s.0.len(), 2);
        assert!(s.0[0].abs() < 1e-6 && s.0[1] > 0.99);
        assert!(Sample::from_wav(b"nope").is_err());
        assert!(Sample::from_wav(&wav(RATE, 1, 24, &[0; 6])).unwrap_err().contains("24 bits"));
    }

    #[test]
    fn plays_add_up_clip_and_finish() {
        let mut m = Mixer::default();
        let tick = Sample::from_pcm(&[0.25, 0.25, 0.25], RATE);
        m.add(&tick, 1.0);
        m.add(&tick, 1.0);
        let mut out = [0i16; 8];
        m.fill(&mut out);
        let one = (0.25 * 32767.0) as i16;
        assert_eq!(&out[..6], &[one * 2; 6]);
        assert_eq!(&out[6..], &[0, 0]);
        assert!(m.voices.is_empty());
        // Loud plays clip instead of wrapping; a silent or bad gain adds nothing.
        m.add(&Sample::from_pcm(&[1.0], RATE), 1.0);
        m.add(&Sample::from_pcm(&[1.0], RATE), 1.0);
        m.add(&tick, 0.0);
        m.add(&tick, f32::NAN);
        assert_eq!(m.voices.len(), 2);
        m.fill(&mut out);
        assert_eq!(out[0], 32767);
    }

    #[test]
    fn the_oldest_voice_makes_room() {
        let mut m = Mixer::default();
        let s = Sample::from_pcm(&[0.1; 10], RATE);
        for _ in 0..MAX_VOICES + 5 {
            m.add(&s, 1.0);
        }
        assert_eq!(m.voices.len(), MAX_VOICES);
    }

    #[test]
    fn a_player_opens_or_says_why_not() {
        // A build machine may have no sound card: either answer is fine, and a
        // player that opened plays without a sound of its own to hear.
        match Player::new() {
            Ok(p) => p.play(&p.load_pcm(&[0.0; 100], 44_100), 0.0),
            Err(e) => assert!(!e.is_empty()),
        }
    }
}
