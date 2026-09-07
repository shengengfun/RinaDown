use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, ErrorKind},
    path::PathBuf,
};

fn main() -> io::Result<()> {
    let manifest_dir = env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(ErrorKind::NotFound, "CARGO_MANIFEST_DIR is not set"))?;
    let locale_dir = manifest_dir.join("../../assets/i18n");
    println!("cargo:rerun-if-changed={}", locale_dir.display());

    let mut locales = BTreeMap::new();
    for entry in fs::read_dir(&locale_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }

        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                io::Error::new(ErrorKind::InvalidData, "locale filename is not UTF-8")
            })?;
        let locale = path
            .file_stem()
            .and_then(|name| name.to_str())
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidData, "locale name is not UTF-8"))?
            .to_ascii_lowercase();

        if !locale
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                format!("unsupported locale filename: {file_name}"),
            ));
        }

        if locales
            .insert(locale.clone(), file_name.to_owned())
            .is_some()
        {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                format!("duplicate locale code: {locale}"),
            ));
        }
    }

    if !locales.iter().any(|(locale, _)| locale == "en") {
        return Err(io::Error::new(
            ErrorKind::NotFound,
            "assets/i18n/en.json is required as the fallback locale",
        ));
    }

    let mut generated = String::from("pub(crate) const EMBEDDED_LOCALES: &[(&str, &str)] = &[\n");
    for (locale, file_name) in locales {
        generated.push_str(&format!(
            "    (\"{locale}\", include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../../assets/i18n/{file_name}\"))),\n"
        ));
    }
    generated.push_str("];\n");

    let output_dir = env::var_os("OUT_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(ErrorKind::NotFound, "OUT_DIR is not set"))?;
    fs::write(output_dir.join("embedded_locales.rs"), generated)
}
