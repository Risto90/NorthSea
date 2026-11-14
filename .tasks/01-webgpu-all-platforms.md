---
id: webgpu-all-platforms
title: WebGPU для всех версий
relationship: blocking|vulkan-native-api
relationship: blocking|webgl-browser-fallback
relationship: sub_issue|vulkan-native-api
relationship: sub_issue|webgl-browser-fallback
subtask: renderer|01. Общий Rust-рендерер WebGPU|Довести общий Rust-рендерер сцены до вывода через WebGPU; не вводить игровой движок и не возвращать программную растеризацию кадров.
subtask: native-platforms|02. WebGPU в Linux и Android|Подключить WebGPU к нативным окнам Linux и Android, сохранив локальный запуск, существующее управление и обработку жизненного цикла платформ.
subtask: browser|03. WebGPU в браузере|Подключить WebGPU к холсту браузерной версии, сохранив полноэкранный режим и автоматический выбор мобильного или настольного интерфейса.
subtask: unavailable-api|04. Обработка недоступности WebGPU|Добавить понятную ошибку, если платформа или устройство не предоставляет WebGPU; резервные API появятся только в связанных задачах.
subtask: verification|05. Проверка Linux, Android и WebAssembly|Проверить сборки Linux, Android и WebAssembly, а также доступные локальные проверки без создания релиза.
---

## Что и зачем
Сделать WebGPU основным графическим API текущих сборок: WebAssembly для сайта и локальные приложения Linux и Android. Общая сцена и игровые правила остаются в Rust, а установленные версии не запускают браузер.

## Готовый результат
- Linux, Android и браузер рисуют одну общую Rust-сцену через WebGPU.
- APK и DEB/RPM запускают локальные приложения; программная растеризация полного кадра не используется.
- В браузере игра запускается без установки и сохраняет отдельные мобильные и настольные элементы управления.
- Если WebGPU недоступен, игра сообщает об этом ясно, не переключаясь молча на другой API.
- Сборки проверены без публикации релиза и без изменения номера версии.

## Вне объёма
- Нативный Vulkan как отдельная альтернативная реализация и настройка выбора между ним и WebGPU.
- WebGL как браузерный резервный API и настройка переключения на него.
- Сборка Windows и выпуск новых релизов.
- Изменение сюжета, игровых механик или пользовательских ассетов.

## Релевантные файлы
- `Cargo.toml`
- `src/android.rs`
- `src/desktop.rs`
- `src/gpu_renderer.rs`
- `src/lib.rs`
- `src/main.rs`
- `src/scene_mesh.rs`
- `src/web.rs`
- `artifacts/north-seas-adventure/public/game.css`
- `scripts/build-android-apk.sh`
- `scripts/build-linux-packages.sh`
- `scripts/build-web.sh`
- `.github/workflows/build-packages.yml`
