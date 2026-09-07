//! FluxDown GPUI 客户端对 Flutter 翻译资源的零复制源复用层。
//!
//! `assets/i18n/*.json` 仍是唯一资源目录；构建脚本自动发现并嵌入语言文件。
//! 查找语义与 Flutter `I18nStore` 一致：locale 精确匹配、主语言匹配、英文
//! 键级回退、空值回退以及 `{name}` 占位插值。

use std::{collections::BTreeMap, sync::Arc};

use thiserror::Error;

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/embedded_locales.rs"));
}

/// Flutter 与 GPUI 共享的最终回退语言。
pub const FALLBACK_LOCALE: &str = "en";

/// 初始桌面 shell 使用的翻译键；完整键集仍可通过字符串访问。
pub mod keys {
    pub const CATEGORY_ALL: &str = "categoryAll";
    pub const CATEGORY_ARCHIVE: &str = "categoryArchive";
    pub const CATEGORY_AUDIO: &str = "categoryAudio";
    pub const CATEGORY_DOCUMENT: &str = "categoryDocument";
    pub const CATEGORY_IMAGE: &str = "categoryImage";
    pub const CATEGORY_OTHER: &str = "categoryOther";
    pub const CATEGORY_PROGRAM: &str = "categoryProgram";
    pub const CATEGORY_VIDEO: &str = "categoryVideo";
    pub const COL_CREATED: &str = "colCreated";
    pub const COL_ETA: &str = "colEta";
    pub const COL_FILE_NAME: &str = "colFileName";
    pub const COL_SIZE: &str = "colSize";
    pub const COL_SPEED: &str = "colSpeed";
    pub const COL_STATUS: &str = "colStatus";
    pub const DELETE: &str = "delete";
    pub const EMPTY_TITLE: &str = "emptyTitle";
    pub const LANGUAGE: &str = "language";
    pub const LANGUAGE_CHINESE: &str = "languageChinese";
    pub const LANGUAGE_DESC: &str = "languageDesc";
    pub const LANGUAGE_ENGLISH: &str = "languageEnglish";
    pub const LATER_QUEUE: &str = "laterQueue";
    pub const MAIN_QUEUE: &str = "mainQueue";
    pub const MENU_FILE: &str = "menuFile";
    pub const MENU_HELP: &str = "menuHelp";
    pub const MENU_ITEMS_PENDING: &str = "menuItemsPending";
    pub const MENU_TASKS: &str = "menuTasks";
    pub const MENU_TOOLS: &str = "menuTools";
    pub const MOBILE_NAV_DOWNLOADS: &str = "mobileNavDownloads";
    pub const PAUSE: &str = "pause";
    pub const RESUME: &str = "resume";
    pub const NEW_DOWNLOAD: &str = "newDownload";
    pub const SETTINGS: &str = "settings";
    pub const SETTINGS_CAT_APPEARANCE: &str = "settingsCatAppearance";
    pub const SIDEBAR_CATEGORY: &str = "sidebarCategory";
    pub const SIDEBAR_QUEUES: &str = "sidebarQueues";
    pub const SIDEBAR_STATUS: &str = "sidebarStatus";
    pub const STATUS_COMPLETED: &str = "statusCompleted";
    pub const STATUS_INCOMPLETE: &str = "statusIncomplete";
    pub const STATUS_DOWNLOADING: &str = "statusDownloading";
    pub const STATUS_ERROR: &str = "statusError";
    pub const VIEW_COLUMNS_AT_LEAST_ONE: &str = "viewColumnsAtLeastOne";
    pub const VIEW_COLUMNS_MENU_TITLE: &str = "viewColumnsMenuTitle";
    pub const VIEW_COLUMNS_RESET_ACTION: &str = "viewColumnsResetAction";
    pub const STATUS_PAUSED: &str = "statusPaused";
    pub const STATUS_SEEDING: &str = "statusSeeding";
    pub const STOP_ALL: &str = "stopAll";
    pub const TAB_ALL: &str = "tabAll";
    pub const THEME_MODE: &str = "themeMode";
    pub const THEME_MODE_DARK: &str = "themeModeDark";
    pub const THEME_MODE_DESC: &str = "themeModeDesc";
    pub const THEME_MODE_LIGHT: &str = "themeModeLight";
    pub const TODAY: &str = "today";
}

/// 嵌入翻译资源无法构造成有效目录时的错误。
#[derive(Debug, Error)]
pub enum I18nError {
    #[error("duplicate embedded locale: {locale}")]
    DuplicateLocale { locale: String },
    #[error("embedded fallback locale '{locale}' is missing")]
    MissingFallback { locale: &'static str },
    #[error("failed to parse embedded locale '{locale}'")]
    InvalidLocale {
        locale: String,
        #[source]
        source: serde_json::Error,
    },
}

/// 已解析的不可变翻译目录；可由多个窗口和 view 共享。
#[derive(Debug)]
pub struct I18nCatalog {
    tables: BTreeMap<String, BTreeMap<String, String>>,
    available: Vec<String>,
}

impl I18nCatalog {
    /// 从 Flutter 的嵌入翻译资源构造目录。
    pub fn load_embedded() -> Result<Self, I18nError> {
        Self::from_sources(embedded::EMBEDDED_LOCALES)
    }

    fn from_sources(sources: &[(&str, &str)]) -> Result<Self, I18nError> {
        let mut tables = BTreeMap::new();
        for (locale, source) in sources {
            let locale = normalize_locale(locale);
            let table =
                serde_json::from_str::<BTreeMap<String, String>>(source).map_err(|source| {
                    I18nError::InvalidLocale {
                        locale: locale.clone(),
                        source,
                    }
                })?;
            if tables.insert(locale.clone(), table).is_some() {
                return Err(I18nError::DuplicateLocale { locale });
            }
        }

        if !tables.contains_key(FALLBACK_LOCALE) {
            return Err(I18nError::MissingFallback {
                locale: FALLBACK_LOCALE,
            });
        }

        let mut available = tables.keys().cloned().collect::<Vec<_>>();
        available.sort_by(|left, right| {
            locale_rank(left)
                .cmp(&locale_rank(right))
                .then_with(|| left.cmp(right))
        });

        Ok(Self { tables, available })
    }

    /// 可用 locale，顺序与 Flutter 一致：`en`、`zh`，随后按代码排序。
    pub fn available_locales(&self) -> &[String] {
        &self.available
    }

    /// 把系统或用户 locale 解析为实际可用代码。
    pub fn resolve_locale<'catalog>(&'catalog self, locale: &str) -> &'catalog str {
        let normalized = normalize_locale(locale);
        if let Some((code, _)) = self.tables.get_key_value(&normalized) {
            return code;
        }

        let prefix = normalized
            .split_once('-')
            .map_or(normalized.as_str(), |(prefix, _)| prefix);
        if let Some((code, _)) = self.tables.get_key_value(prefix) {
            return code;
        }

        for code in &self.available {
            let code_prefix = code
                .split_once('-')
                .map_or(code.as_str(), |(prefix, _)| prefix);
            if code_prefix == prefix {
                return code;
            }
        }

        FALLBACK_LOCALE
    }

    /// 返回语言文件声明的自称；缺失时返回 locale 代码。
    pub fn native_name<'catalog>(&'catalog self, locale: &'catalog str) -> &'catalog str {
        self.tables
            .get(locale)
            .and_then(|table| table.get("languageNativeName"))
            .filter(|value| !value.is_empty())
            .map_or(locale, String::as_str)
    }

    /// 创建持有本目录的翻译器，避免每次渲染重复解析 locale。
    pub fn translator(self: &Arc<Self>, locale: &str) -> Translator {
        Translator {
            catalog: Arc::clone(self),
            locale: self.resolve_locale(locale).to_owned(),
        }
    }

    fn lookup_resolved<'catalog>(
        &'catalog self,
        locale: &str,
        key: &'catalog str,
    ) -> &'catalog str {
        self.tables
            .get(locale)
            .and_then(|table| table.get(key))
            .filter(|value| !value.is_empty())
            .or_else(|| {
                self.tables
                    .get(FALLBACK_LOCALE)
                    .and_then(|table| table.get(key))
                    .filter(|value| !value.is_empty())
            })
            .map_or(key, String::as_str)
    }
}

/// 已解析 locale 的轻量翻译器；普通查表不分配。
#[derive(Clone, Debug)]
pub struct Translator {
    catalog: Arc<I18nCatalog>,
    locale: String,
}

impl Translator {
    /// 当前实际 locale 代码。
    pub fn locale(&self) -> &str {
        &self.locale
    }

    /// 切换 locale；返回实际 locale 是否变化。
    pub fn set_locale(&mut self, locale: &str) -> bool {
        let resolved = self.catalog.resolve_locale(locale);
        if self.locale == resolved {
            return false;
        }
        self.locale.clear();
        self.locale.push_str(resolved);
        true
    }

    /// 查表；当前语言空值/缺键时回退英文，再回退键名。
    pub fn text<'translator>(&'translator self, key: &'translator str) -> &'translator str {
        self.catalog.lookup_resolved(&self.locale, key)
    }

    /// 查表并执行 `{name}` 占位插值；仅参数化字符串发生分配。
    pub fn text_with(&self, key: &str, arguments: &[(&str, &str)]) -> String {
        let mut value = self.text(key).to_owned();
        for (name, replacement) in arguments {
            let mut placeholder = String::with_capacity(name.len() + 2);
            placeholder.push('{');
            placeholder.push_str(name);
            placeholder.push('}');
            value = value.replace(&placeholder, replacement);
        }
        value
    }

    /// 当前语言的自称。
    pub fn native_name(&self) -> &str {
        self.catalog.native_name(&self.locale)
    }

    /// 目录中的全部可用 locale。
    pub fn available_locales(&self) -> &[String] {
        self.catalog.available_locales()
    }

    /// 任一可用 locale 的自称。
    pub fn native_name_of<'translator>(
        &'translator self,
        locale: &'translator str,
    ) -> &'translator str {
        self.catalog.native_name(locale)
    }
}

fn normalize_locale(locale: &str) -> String {
    locale.trim().to_ascii_lowercase().replace('_', "-")
}

fn locale_rank(locale: &str) -> u8 {
    match locale {
        "en" => 0,
        "zh" => 1,
        _ => 2,
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, sync::Arc};

    use super::{I18nCatalog, I18nError};

    #[test]
    fn resolves_exact_prefix_and_fallback_locales() -> Result<(), I18nError> {
        let catalog = I18nCatalog::load_embedded()?;

        assert_eq!(catalog.resolve_locale("zh_CN"), "zh");
        assert_eq!(catalog.resolve_locale("en-US"), "en");
        assert_eq!(catalog.resolve_locale("not-a-locale"), "en");
        Ok(())
    }

    #[test]
    fn falls_back_per_key_and_interpolates() -> Result<(), I18nError> {
        let catalog = Arc::new(I18nCatalog::from_sources(&[
            (
                "en",
                r#"{"languageNativeName":"English","hello":"Hello {name}","fallback":"Fallback"}"#,
            ),
            (
                "zh",
                r#"{"languageNativeName":"中文","hello":"你好，{name}","fallback":""}"#,
            ),
        ])?);
        let translator = catalog.translator("zh-CN");

        assert_eq!(
            translator.text_with("hello", &[("name", "FluxDown")]),
            "你好，FluxDown"
        );
        assert_eq!(translator.text("fallback"), "Fallback");
        assert_eq!(translator.text("missing"), "missing");
        Ok(())
    }

    #[test]
    fn flutter_baseline_locales_have_matching_keys() -> Result<(), I18nError> {
        let catalog = I18nCatalog::load_embedded()?;
        let english = catalog
            .tables
            .get("en")
            .map(|table| table.keys().map(String::as_str).collect::<BTreeSet<_>>());
        let chinese = catalog
            .tables
            .get("zh")
            .map(|table| table.keys().map(String::as_str).collect::<BTreeSet<_>>());

        assert_eq!(english, chinese);
        Ok(())
    }
}
