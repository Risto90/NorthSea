use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PACKAGE_ID: &str = "com.northseas.adventure";
const DATA_MARKER: &str = "north-sea-upgrade-data-preserved";

fn required_path(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("{name} is not set")))
}

fn command_output(program: &Path, args: &[&str]) -> Output {
    let output = Command::new(program)
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("Не удалось запустить {}: {error}", program.display()));
    assert!(
        output.status.success(),
        "Команда {} завершилась с ошибкой: {}",
        program.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn metadata(apk: &Path) -> (String, u32) {
    let build_tools = required_path("ANDROID_BUILD_TOOLS");
    let aapt = build_tools.join("aapt");
    let apk = apk.to_str().expect("Путь APK должен быть UTF-8");
    let output = command_output(&aapt, &["dump", "badging", apk]);
    let text = String::from_utf8(output.stdout).expect("Вывод aapt должен быть UTF-8");
    let package_line = text
        .lines()
        .find(|line| line.starts_with("package: "))
        .expect("В APK отсутствуют сведения о пакете");
    let package_id = package_line
        .split("name='")
        .nth(1)
        .and_then(|value| value.split('\'').next())
        .expect("В APK отсутствует application ID")
        .to_owned();
    let version_code = package_line
        .split("versionCode='")
        .nth(1)
        .and_then(|value| value.split('\'').next())
        .expect("В APK отсутствует versionCode")
        .parse()
        .expect("versionCode должен быть числом");
    (package_id, version_code)
}

fn certificate(apk: &Path) -> String {
    let build_tools = required_path("ANDROID_BUILD_TOOLS");
    let apksigner = build_tools.join("apksigner");
    let apk = apk.to_str().expect("Путь APK должен быть UTF-8");
    let output = command_output(&apksigner, &["verify", "--print-certs", apk]);
    let text = String::from_utf8(output.stdout).expect("Вывод apksigner должен быть UTF-8");
    text.lines()
        .find_map(|line| line.strip_prefix("Signer #1 certificate SHA-256 digest: "))
        .expect("В APK отсутствует сертификат подписи")
        .trim()
        .to_ascii_lowercase()
}

fn adb(args: &[&str]) -> Output {
    command_output(Path::new("adb"), args)
}

#[test]
fn both_apks_keep_the_same_package_and_have_increasing_version_codes() {
    let baseline = required_path("ANDROID_BASELINE_APK");
    let candidate = required_path("ANDROID_CANDIDATE_APK");
    let expected_baseline: u32 = std::env::var("ANDROID_BASELINE_VERSION_CODE")
        .expect("ANDROID_BASELINE_VERSION_CODE is not set")
        .parse()
        .expect("Базовый versionCode должен быть числом");
    let expected_candidate: u32 = std::env::var("ANDROID_CANDIDATE_VERSION_CODE")
        .expect("ANDROID_CANDIDATE_VERSION_CODE is not set")
        .parse()
        .expect("Новый versionCode должен быть числом");

    let (baseline_package, baseline_code) = metadata(&baseline);
    let (candidate_package, candidate_code) = metadata(&candidate);
    assert_eq!(baseline_package, PACKAGE_ID);
    assert_eq!(candidate_package, PACKAGE_ID);
    assert_eq!(baseline_code, expected_baseline);
    assert_eq!(candidate_code, expected_candidate);
    assert!(candidate_code > baseline_code);
}

#[test]
fn both_apks_are_signed_with_the_same_persistent_key() {
    let baseline = required_path("ANDROID_BASELINE_APK");
    let candidate = required_path("ANDROID_CANDIDATE_APK");
    assert_eq!(certificate(&baseline), certificate(&candidate));
}

#[test]
fn installing_the_update_preserves_private_application_data() {
    let baseline = required_path("ANDROID_BASELINE_APK");
    let candidate = required_path("ANDROID_CANDIDATE_APK");
    let baseline = baseline.to_str().expect("Путь APK должен быть UTF-8");
    let candidate = candidate.to_str().expect("Путь APK должен быть UTF-8");

    let _ = adb(&["uninstall", PACKAGE_ID]);
    adb(&["install", baseline]);
    adb(&[
        "shell",
        "run-as",
        PACKAGE_ID,
        "sh",
        "-c",
        "mkdir -p files && printf 'north-sea-upgrade-data-preserved' > files/upgrade-check",
    ]);
    adb(&["install", "-r", candidate]);

    let output = adb(&["shell", "run-as", PACKAGE_ID, "cat", "files/upgrade-check"]);
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), DATA_MARKER);
}
