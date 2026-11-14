use std::fs;
use std::path::{Path, PathBuf};

use crate::GraphicsApiPreference;

pub(crate) struct NativeSettings {
    path: PathBuf,
    graphics_api: GraphicsApiPreference,
}

impl NativeSettings {
    pub(crate) fn load(path: PathBuf) -> Result<Self, String> {
        let graphics_api = match fs::read_to_string(&path) {
            Ok(value) => GraphicsApiPreference::from_storage_value(&value).ok_or_else(|| {
                format!(
                    "В файле настроек графики {} указан неизвестный режим.",
                    path.display()
                )
            })?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                GraphicsApiPreference::Automatic
            }
            Err(error) => {
                return Err(format!(
                    "Не удалось прочитать настройки графики {}: {error}",
                    path.display()
                ));
            }
        };
        Ok(Self { path, graphics_api })
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn load_linux() -> Result<Self, String> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .map(|home| home.join(".config"))
            })
            .ok_or_else(|| {
                "Не задан XDG_CONFIG_HOME или HOME для сохранения настроек графики.".to_owned()
            })?;
        Self::load(base.join("north-seas-adventure").join("graphics-api"))
    }

    #[cfg(target_os = "android")]
    pub(crate) fn load_android(app: &android_activity::AndroidApp) -> Result<Self, String> {
        let base = app
            .internal_data_path()
            .ok_or_else(|| "Android не предоставил внутреннюю папку настроек.".to_owned())?;
        Self::load(base.join("graphics-api"))
    }

    pub(crate) fn graphics_api(&self) -> GraphicsApiPreference {
        self.graphics_api
    }

    pub(crate) fn save_graphics_api(
        &mut self,
        graphics_api: GraphicsApiPreference,
    ) -> Result<(), String> {
        if self.graphics_api == graphics_api {
            return Ok(());
        }
        let parent = self.path.parent().ok_or_else(|| {
            format!(
                "Не удалось определить папку файла настроек {}.",
                self.path.display()
            )
        })?;
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Не удалось создать папку настроек графики {}: {error}",
                parent.display()
            )
        })?;
        let temporary_path = temporary_path(&self.path);
        fs::write(&temporary_path, graphics_api.as_storage_value()).map_err(|error| {
            format!(
                "Не удалось записать временный файл настроек графики {}: {error}",
                temporary_path.display()
            )
        })?;
        fs::rename(&temporary_path, &self.path).map_err(|error| {
            let _ = fs::remove_file(&temporary_path);
            format!(
                "Не удалось сохранить настройки графики {}: {error}",
                self.path.display()
            )
        })?;
        self.graphics_api = graphics_api;
        Ok(())
    }
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut temporary_path = path.as_os_str().to_owned();
    temporary_path.push(".tmp");
    PathBuf::from(temporary_path)
}

#[cfg(test)]
mod tests {
    use super::NativeSettings;
    use crate::GraphicsApiPreference;

    #[test]
    fn missing_settings_file_defaults_to_automatic_and_selection_persists() {
        let path = std::env::temp_dir().join(format!(
            "north-seas-graphics-settings-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let mut settings = NativeSettings::load(path.clone()).expect("настройки по умолчанию");
        assert_eq!(settings.graphics_api(), GraphicsApiPreference::Automatic);
        settings
            .save_graphics_api(GraphicsApiPreference::Vulkan)
            .expect("сохранение режима Vulkan");
        let loaded = NativeSettings::load(path.clone()).expect("чтение сохранённых настроек");
        assert_eq!(loaded.graphics_api(), GraphicsApiPreference::Vulkan);
        std::fs::remove_file(path).expect("удаление временных настроек");
    }

    #[test]
    fn unknown_saved_mode_is_reported_instead_of_silently_reset() {
        let path = std::env::temp_dir().join(format!(
            "north-seas-invalid-graphics-settings-{}",
            std::process::id()
        ));
        std::fs::write(&path, "other-api").expect("создание некорректной настройки");
        assert!(NativeSettings::load(path.clone()).is_err());
        std::fs::remove_file(path).expect("удаление временной настройки");
    }
}
