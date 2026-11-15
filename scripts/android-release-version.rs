use std::env;
use std::process;

const APK_ID: u32 = 1 << 24;
const MAX_ANDROID_VERSION_CODE: u32 = APK_ID | 0x00ff_ffff;

fn validate_release_tag(tag: &str) -> Result<(), String> {
    let parts: Vec<&str> = tag.split('.').collect();
    if parts.len() != 4 || parts.iter().any(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit())) {
        return Err(format!("Неверный номер релиза: {tag}"));
    }

    let month_year = parts[1];
    if month_year.len() != 4 {
        return Err(format!("Месяц и год должны иметь формат mmyy: {tag}"));
    }
    let month: u8 = month_year[..2]
        .parse()
        .map_err(|_| format!("Неверный месяц в номере релиза: {tag}"))?;
    if !(1..=12).contains(&month) {
        return Err(format!("Месяц в номере релиза должен быть от 01 до 12: {tag}"));
    }

    let commit_count: u64 = parts[3]
        .parse()
        .map_err(|_| format!("Неверное число коммитов в номере релиза: {tag}"))?;
    if commit_count == 0 {
        return Err(format!("Число коммитов в номере релиза должно быть не меньше 1: {tag}"));
    }
    Ok(())
}

fn cargo_version_for_code(version_code: u32, release_tag: &str) -> Result<String, String> {
    validate_release_tag(release_tag)?;
    if !(APK_ID..=MAX_ANDROID_VERSION_CODE).contains(&version_code) {
        return Err(format!(
            "versionCode {version_code} выходит за диапазон cargo-apk с APK ID 1"
        ));
    }

    let major = (version_code >> 16) & 0xff;
    let minor = (version_code >> 8) & 0xff;
    let patch = version_code & 0xff;
    Ok(format!("{major}.{minor}.{patch}+{release_tag}"))
}

fn next_release_version(previous_code: u32, release_tag: &str) -> Result<(u32, String), String> {
    if previous_code >= MAX_ANDROID_VERSION_CODE {
        return Err("Исчерпан диапазон versionCode, который поддерживает cargo-apk.".to_owned());
    }
    let next_code = previous_code + 1;
    let cargo_version = cargo_version_for_code(next_code, release_tag)?;
    Ok((next_code, cargo_version))
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.as_slice() {
        [mode, release_tag, version_code] if mode == "for-code" => {
            let version_code = version_code
                .parse()
                .map_err(|_| format!("Неверный versionCode: {version_code}"))?;
            let package_version = cargo_version_for_code(version_code, release_tag)?;
            println!("{version_code}\t{package_version}");
        }
        [release_tag, previous_code] => {
            let previous_code = previous_code
                .parse()
                .map_err(|_| format!("Неверный предыдущий versionCode: {previous_code}"))?;
            let (version_code, package_version) =
                next_release_version(previous_code, release_tag)?;
            println!("{version_code}\t{package_version}");
        }
        _ => {
            return Err(
                "Использование: android-release-version <номер-релиза> <предыдущий-versionCode> \
                 | for-code <номер-релиза> <versionCode>"
                    .to_owned(),
            );
        }
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn increments_from_the_published_apk_version_code() {
        let (code, package_version) =
            next_release_version(16_777_472, "0.1126.3.1").unwrap();
        assert_eq!(code, 16_777_473);
        assert_eq!(package_version, "0.1.1+0.1126.3.1");
    }

    #[test]
    fn preserves_the_previous_code_for_the_upgrade_test_baseline() {
        let package_version = cargo_version_for_code(16_777_472, "0.1126.3.1").unwrap();
        assert_eq!(package_version, "0.1.0+0.1126.3.1");
    }

    #[test]
    fn carries_patch_overflow_into_the_minor_component() {
        let (code, package_version) =
            next_release_version(16_777_727, "0.1126.3.1").unwrap();
        assert_eq!(code, 16_777_728);
        assert_eq!(package_version, "0.2.0+0.1126.3.1");
    }

    #[test]
    fn carries_minor_overflow_into_the_major_component() {
        let previous = (APK_ID | (255 << 8) | 255) as u32;
        let (code, package_version) = next_release_version(previous, "0.1126.3.1").unwrap();
        assert_eq!(code, APK_ID | (1 << 16));
        assert_eq!(package_version, "1.0.0+0.1126.3.1");
    }

    #[test]
    fn rejects_invalid_release_tags_and_exhausted_codes() {
        assert!(cargo_version_for_code(16_777_472, "0.1326.3.1").is_err());
        assert!(next_release_version(MAX_ANDROID_VERSION_CODE, "0.1126.3.1").is_err());
    }
}
