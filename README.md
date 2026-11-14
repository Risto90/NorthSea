# Моря Туманного Севера

Морское приключение о Торфинне Рататоске. Игровая логика написана на Rust.

## GitHub Actions

- Каждый push в `main` собирает Android APK и пакеты Linux `.deb`/`.rpm`.
- Готовые файлы доступны в разделе **Artifacts** запуска GitHub Actions в течение 90 дней.
- WebAssembly-версия автоматически публикуется на GitHub Pages после push в `main`.

Android APK подписывается тестовым ключом и предназначен для установки и проверки, не для публикации в Google Play.

Пакеты Linux устанавливают Rust-сервер и WebAssembly-файлы игры. Запуск из меню открывает игру в системном браузере; для него нужны Bash, `xdg-utils` и браузер.

## Сборка вручную

```sh
./scripts/with-rust-toolchain.sh cargo test
./scripts/with-rust-toolchain.sh cargo build --release
bash scripts/build-web.sh
bash scripts/build-linux-packages.sh
bash scripts/build-android-apk.sh
```
