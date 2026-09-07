use fluxdown_ui_i18n::{Translator, keys};
use gpui::SharedString;

#[derive(Clone)]
pub(crate) struct ShellStrings {
    pub(crate) menu_file: SharedString,
    pub(crate) menu_help: SharedString,
    pub(crate) menu_items_pending: SharedString,
    pub(crate) menu_tasks: SharedString,
    pub(crate) menu_tools: SharedString,
}

impl ShellStrings {
    pub(crate) fn from_translator(translator: &Translator) -> Self {
        Self {
            menu_file: shared(translator.text(keys::MENU_FILE)),
            menu_help: shared(translator.text(keys::MENU_HELP)),
            menu_items_pending: shared(translator.text(keys::MENU_ITEMS_PENDING)),
            menu_tasks: shared(translator.text(keys::MENU_TASKS)),
            menu_tools: shared(translator.text(keys::MENU_TOOLS)),
        }
    }
}

fn shared(value: &str) -> SharedString {
    SharedString::from(value.to_owned())
}
