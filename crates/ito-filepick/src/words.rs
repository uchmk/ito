//! The dialog's words. The model has none; the window asks here.

use crate::model::{InputError, Kind, NameFault};
use crate::path::PathError;

/// Every string the window shows. Build with [`Words::en`], [`Words::ja`] or [`Words::for_lang`], or fill the fields yourself.
#[derive(Clone, Debug)]
pub struct Words {
    pub open_file: String,
    pub open_files: String,
    pub choose_folder: String,
    pub open: String,
    pub choose: String,
    pub cancel: String,
    pub close: String,
    pub back: String,
    pub forward: String,
    pub up: String,
    pub refresh: String,
    pub edit_path: String,
    pub path: String,
    pub places: String,
    pub name: String,
    pub folder_name: String,
    pub file_name: String,
    pub all_files: String,
    pub show_hidden: String,
    pub col_name: String,
    pub col_modified: String,
    pub col_kind: String,
    pub col_size: String,
    pub folder: String,
    pub file: String,
    pub other: String,
    pub unknown: String,
    /// `{ext}` is the upper-case extension.
    pub ext_file: String,
    pub loading: String,
    pub empty: String,
    pub empty_hint: String,
    pub no_match: String,
    pub no_match_hint: String,
    pub failed: String,
    pub retry: String,
    /// `{n}` items.
    pub truncated: String,
    /// `{n}` items.
    pub left_out: String,
    /// `{n}`.
    pub selected_n: String,
    /// `{name}`.
    pub not_found: String,
    /// `{name}`, `{n}`.
    pub not_among: String,
    pub choose_folder_err: String,
    pub cannot_open: String,
    pub one_name: String,
    /// The title of the save dialog.
    pub save_file: String,
    /// The save dialog's main button.
    pub save: String,
    /// The red button of the replace question.
    pub replace: String,
    /// `{name}`.
    pub replace_title: String,
    pub replace_body: String,
    /// `{name}`.
    pub cannot_replace: String,
    /// `{name}`.
    pub ambiguous: String,
    /// `{name}`.
    pub bad_char: String,
    /// `{name}`.
    pub reserved_name: String,
    pub trailing: String,
    pub no_share: String,
    pub drive_relative: String,
    pub verbatim: String,
    pub nul: String,
}

impl Words {
    pub fn en() -> Self {
        let s = |t: &str| t.to_owned();
        Words {
            open_file: s("Open file"),
            open_files: s("Open files"),
            choose_folder: s("Choose folder"),
            open: s("Open"),
            choose: s("Choose"),
            cancel: s("Cancel"),
            close: s("Close"),
            back: s("Back"),
            forward: s("Forward"),
            up: s("Up one folder"),
            refresh: s("Refresh"),
            edit_path: s("Type a path"),
            path: s("Path"),
            places: s("Places"),
            name: s("Name"),
            folder_name: s("Folder"),
            file_name: s("File name"),
            all_files: s("All files"),
            show_hidden: s("Show hidden items"),
            col_name: s("Name"),
            col_modified: s("Modified"),
            col_kind: s("Kind"),
            col_size: s("Size"),
            folder: s("Folder"),
            file: s("File"),
            other: s("Special file"),
            unknown: s("Unknown"),
            ext_file: s("{ext} file"),
            loading: s("Loading…"),
            empty: s("This folder is empty"),
            empty_hint: s("Nothing to show here."),
            no_match: s("No items match the filter"),
            no_match_hint: s("Change the filter or show hidden items."),
            failed: s("Cannot open this folder"),
            retry: s("Try again"),
            truncated: s("Showing the first {n} items; the folder has more."),
            left_out: s("{n} items could not be shown."),
            selected_n: s("{n} selected"),
            not_found: s("\"{name}\" was not found in this folder."),
            not_among: s("\"{name}\" is not among the {n} items shown. Type a folder's path to open it."),
            choose_folder_err: s("Choose a folder, not a file."),
            cannot_open: s("This item cannot be opened."),
            one_name: s("Type one name."),
            save_file: s("Save as"),
            save: s("Save"),
            replace: s("Replace"),
            replace_title: s("Replace \"{name}\"?"),
            replace_body: s("A file with this name is already in this folder. Saving replaces it."),
            cannot_replace: s("\"{name}\" is not a file, so it cannot be replaced."),
            ambiguous: s("Several items match \"{name}\". Type the name exactly as the list shows it."),
            bad_char: s("\"{name}\" has a character that cannot be in a file name (: * ? \" < > |)."),
            reserved_name: s("\"{name}\" is a name Windows keeps for a device. Choose another name."),
            trailing: s("A file name cannot end with a dot."),
            no_share: s("A network path needs a server and a share."),
            drive_relative: s("A drive-relative path is not supported. Add a backslash after the drive."),
            verbatim: s("Paths starting with \\\\?\\ or \\\\.\\ are not supported."),
            nul: s("A path cannot contain a null character."),
        }
    }

    pub fn ja() -> Self {
        let s = |t: &str| t.to_owned();
        Words {
            open_file: s("ファイルを開く"),
            open_files: s("ファイルを開く（複数）"),
            choose_folder: s("フォルダーを選ぶ"),
            open: s("開く"),
            choose: s("選ぶ"),
            cancel: s("キャンセル"),
            close: s("閉じる"),
            back: s("戻る"),
            forward: s("進む"),
            up: s("1 つ上のフォルダー"),
            refresh: s("再読み込み"),
            edit_path: s("パスを入力"),
            path: s("パス"),
            places: s("よく使う場所"),
            name: s("名前"),
            folder_name: s("フォルダー"),
            file_name: s("ファイル名"),
            all_files: s("すべてのファイル"),
            show_hidden: s("隠しファイルを表示"),
            col_name: s("名前"),
            col_modified: s("更新日時"),
            col_kind: s("種類"),
            col_size: s("サイズ"),
            folder: s("フォルダー"),
            file: s("ファイル"),
            other: s("特殊ファイル"),
            unknown: s("不明"),
            ext_file: s("{ext} ファイル"),
            loading: s("読み込み中…"),
            empty: s("このフォルダーは空です"),
            empty_hint: s("表示するものがありません。"),
            no_match: s("フィルターに合うものがありません"),
            no_match_hint: s("フィルターを変えるか、隠しファイルを表示してください。"),
            failed: s("このフォルダーを開けません"),
            retry: s("もう一度試す"),
            truncated: s("先頭の {n} 件を表示しています。フォルダーにはまだあります。"),
            left_out: s("{n} 件は表示できませんでした。"),
            selected_n: s("{n} 件を選択"),
            not_found: s("「{name}」が見つかりません。"),
            not_among: s("「{name}」は表示している {n} 件の中にありません。フォルダーのパスを打つと、そこを開きます。"),
            choose_folder_err: s("ファイルではなくフォルダーを選んでください。"),
            cannot_open: s("この項目は開けません。"),
            one_name: s("名前は 1 つだけ入力してください。"),
            save_file: s("名前を付けて保存"),
            save: s("保存"),
            replace: s("置き換える"),
            replace_title: s("「{name}」を置き換えますか？"),
            replace_body: s("このフォルダーには同じ名前のファイルが既にあります。保存すると置き換わります。"),
            cannot_replace: s("「{name}」はファイルではないので、置き換えられません。"),
            ambiguous: s("「{name}」に合う項目がいくつもあります。一覧のとおりに（大文字と小文字も）打ってください。"),
            bad_char: s("「{name}」には、ファイル名に使えない文字（: * ? \" < > |）が入っています。"),
            reserved_name: s("「{name}」は Windows が装置に使う名前です。別の名前にしてください。"),
            trailing: s("ファイル名の終わりに「.」は使えません。"),
            no_share: s("ネットワークのパスにはサーバーと共有名が必要です。"),
            drive_relative: s("ドライブだけの相対パスは使えません。ドライブの後ろに \\ を付けてください。"),
            verbatim: s("\\\\?\\ や \\\\.\\ で始まるパスは使えません。"),
            nul: s("パスに null 文字は使えません。"),
        }
    }

    /// `"ja"` gives Japanese; anything else English.
    pub fn for_lang(lang: &str) -> Self {
        if lang == "ja" {
            Self::ja()
        } else {
            Self::en()
        }
    }

    /// The window title for the mode.
    pub fn title(&self, mode: crate::Mode) -> &str {
        match mode {
            crate::Mode::OpenFile => &self.open_file,
            crate::Mode::OpenFiles => &self.open_files,
            crate::Mode::OpenFolder => &self.choose_folder,
            crate::Mode::SaveFile => &self.save_file,
        }
    }

    /// The text for something wrong with what was typed.
    pub fn error(&self, e: &InputError) -> String {
        match e {
            InputError::NotFound { name } => self.not_found.replace("{name}", name),
            // The count goes in first and the name last, so a name such as `{n}` is never read again.
            InputError::NotAmongShown { name, shown } => self.not_among.replace("{n}", &shown.to_string()).replace("{name}", name),
            InputError::ChooseFolder => self.choose_folder_err.clone(),
            InputError::CannotOpen => self.cannot_open.clone(),
            InputError::OneName => self.one_name.clone(),
            InputError::BadName { name, why } => match why {
                NameFault::Char => self.bad_char.replace("{name}", name),
                NameFault::Reserved => self.reserved_name.replace("{name}", name),
                NameFault::Trailing => self.trailing.clone(),
            },
            InputError::CannotReplace { name } => self.cannot_replace.replace("{name}", name),
            InputError::Ambiguous { name } => self.ambiguous.replace("{name}", name),
            InputError::Path(p) => self.path_error(p),
        }
    }

    pub fn path_error(&self, e: &PathError) -> String {
        match e {
            PathError::NoShare => self.no_share.clone(),
            PathError::DriveRelative => self.drive_relative.clone(),
            PathError::Verbatim => self.verbatim.clone(),
            PathError::Nul => self.nul.clone(),
        }
    }

    /// The Kind column's text: the extension in capitals for a file that has one.
    pub fn kind_word(&self, kind: Kind, ext: Option<&str>) -> String {
        match kind {
            Kind::Dir => self.folder.clone(),
            Kind::File => match ext {
                Some(x) => self.ext_file.replace("{ext}", &x.to_uppercase()),
                None => self.file.clone(),
            },
            Kind::Other => self.other.clone(),
            Kind::Unknown => self.unknown.clone(),
        }
    }

    pub fn first_n(&self, n: usize) -> String {
        self.truncated.replace("{n}", &n.to_string())
    }

    pub fn left_out_n(&self, n: u32) -> String {
        self.left_out.replace("{n}", &n.to_string())
    }
}

impl Default for Words {
    fn default() -> Self {
        Self::en()
    }
}
