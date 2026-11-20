#!/usr/bin/env bash
# Makes the JPEG XL fixtures of ito-image's tests (feature `jxl`) from tiny PNGs.
# Needs ImageMagick 7 with libjxl (made with 7.1.2 / libjxl 0.11.2) and python3. CI does not run this: the files are committed.
# Run from this folder:  bash make-jxl.sh
set -euo pipefail
cd "$(dirname "$0")"
M="${MAGICK:-magick}"
[ -x "$M" ] || command -v "$M" >/dev/null || M="/c/Program Files/ImageMagick-7.1.2-Q16-HDRI/magick"

# 16x8 source: 8 blocks of 4x4 with distinct colours.
colors=("#e03020" "#20c040" "#2040e0" "#f0e020" "#e020e0" "#20e0e0" "#808080" "#401060")
args=()
i=0
for y in 0 4; do for x in 0 4 8 12; do
  args+=(-fill "${colors[$i]}" -draw "rectangle $x,$y $((x+3)),$((y+3))"); i=$((i+1))
done; done
"$M" -size 16x8 xc:black "${args[@]}" -depth 8 -define png:color-type=2 rgb.png

# Same blocks with alpha 255, 200, 128, 64 repeating by column (no fully transparent pixel: its colour is not kept).
alphas=(255 200 128 64)
margs=()
for x in $(seq 0 15); do margs+=(-fill "gray(${alphas[$((x % 4))]})" -draw "point $x,0" -draw "rectangle $x,0 $x,7"); done
"$M" -size 16x8 xc:white "${margs[@]}" -depth 8 -define png:color-type=0 alpha-mask.png
"$M" rgb.png alpha-mask.png -alpha off -compose CopyOpacity -composite -define png:color-type=6 -depth 8 rgba.png
rm alpha-mask.png

"$M" rgb.png -colorspace Gray -depth 8 -define png:color-type=0 gray.png
for c in red green blue; do "$M" -size 16x8 xc:$c -depth 8 -define png:color-type=2 solid-$c.png; done

# 1. lossless RGB
"$M" rgb.png -define jxl:effort=7 -quality 100 lossless.jxl
# 2. lossy (VarDCT)
"$M" rgb.png -quality 90 lossy.jxl
# 3. lossless with alpha (straight)
"$M" rgba.png -quality 100 alpha.jxl
# 4. animation of three solid frames (red, green, blue); frame 0 is red
"$M" -delay 10 solid-red.png solid-green.png solid-blue.png -loop 0 -quality 100 anim.jxl
# 5. 16-bit lossless
"$M" rgb.png -depth 16 -quality 100 deep16.jxl
# 6. grayscale lossless
"$M" gray.png -quality 100 gray.jxl
# 7. orientation 6 (rotate 90 clockwise to display): the pixels are stored unrotated, the header says "rotate"
"$M" rgb.png -orient right-top -quality 100 orient6.jxl
# 8. embedded ICC (custom P3-primaries profile, curve table, so libjxl cannot name it)
python3 make-icc.py 2>/dev/null || python make-icc.py
"$M" rgb.png -profile p3-test.icc -quality 100 icc.jxl

"$M" identify -format "%f %w x %h, %n frame(s), orientation %[orientation]\n" lossless.jxl lossy.jxl alpha.jxl anim.jxl deep16.jxl gray.jxl orient6.jxl icc.jxl
ls -l *.jxl
