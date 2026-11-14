---
id: vulkan-native-api
title: Vulkan для Linux и Android
relationship: blocked_by|webgpu-all-platforms
relationship: relates_to|webgl-browser-fallback
subtask: vulkan-renderer|01. Отдельный Vulkan-путь|Добавить отдельный низкоуровневый Vulkan-путь на Rust для локальных окон Linux и Android, сохранив общий меш и игровые правила.
subtask: hardware-detection|02. Выбор API по модели GPU|При запуске определять точную модель GPU. Прямой Vulkan безусловно выбирать для AMD Radeon RX 400, RX 500, RX Vega 56 и RX Vega 64: AMD описывает эти семейства как рассчитанные или оптимизированные для низкоуровневых API, включая Vulkan. Не относить другие модели к этому классу по производителю или совместимости Vulkan-драйвера. На остальных GPU выбирать WebGPU, если он доступен; Vulkan — только при недоступном WebGPU. При отсутствии обоих API сообщать ошибку. Проверить выбор и классификацию Rust-тестами.
subtask: api-settings|03. Настройки графического API|Добавить настройки API с режимами автоматического выбора, только Vulkan и только WebGPU; сохранять выбор пользователя.
subtask: explicit-errors|04. Ошибки выбранного API|Показывать понятную ошибку, когда принудительно выбранный API недоступен, и не менять режим без согласия пользователя.
subtask: vulkan-tests|05. Проверка Vulkan и WebGPU|Проверить сборки и обработку сценариев «Vulkan доступен», «Vulkan недоступен» и «API выбран вручную» на доступных тестовых устройствах и в автоматических проверках.
---

## Что и зачем
Добавить Vulkan как отдельный нативный графический API для Linux и Android. WebGPU остаётся обычным API по умолчанию. При запуске игра определяет точную модель GPU. Безусловный приоритет прямого Vulkan действует только для AMD Radeon RX 400, RX 500, RX Vega 56 и RX Vega 64, которые AMD отдельно описывает в контексте низкоуровневых API, включая Vulkan. Для других моделей сам бренд AMD или совместимость Vulkan-драйвера не дают такого приоритета: игра использует WebGPU, если он доступен, и прямой Vulkan только при недоступности WebGPU. Пользователь может принудительно выбрать Vulkan или WebGPU в настройках.

## Готовый результат
- Linux и Android при запуске определяют точную модель GPU.
- Для AMD Radeon RX 400, RX 500, RX Vega 56 и RX Vega 64 нативная версия всегда использует прямой Vulkan, даже если доступен WebGPU.
- Для прочих моделей сам факт поддержки Vulkan не меняет обычный выбор WebGPU; Vulkan используется только если WebGPU недоступен и GPU сообщает о поддержке Vulkan.
- Производитель AMD, совместимость с Vulkan-драйвером или успешная инициализация Vulkan по отдельности не заменяют классификацию модели.
- Если ни WebGPU, ни Vulkan недоступны для выбранного режима, игра показывает понятную ошибку.
- В настройках нативной игры доступны режимы «Автоматически», «Только Vulkan» и «Только WebGPU»; выбор сохраняется.
- Принудительный режим Vulkan не переключается незаметно на WebGPU, если адаптер не поддерживает Vulkan, и показывает понятное сообщение.
- В диагностике можно определить выбранный API и графический адаптер.
- Rust-тестами проверены классификация моделей и выбор между WebGPU и Vulkan; проверены нативные сборки Linux и Android без выпуска нового релиза.

## Вне объёма
- WebGL и настройки графики в браузере.
- Сборка Windows.
- Игровой движок и автоматическое создание новых релизов.

## Релевантные файлы
- `Cargo.toml`
- `src/android.rs`
- `src/desktop.rs`
- `src/gpu_renderer.rs`
- `src/lib.rs`
- `src/scene_mesh.rs`
- `scripts/build-android-apk.sh`
- `scripts/build-linux-packages.sh`
- `.github/workflows/build-packages.yml`

## Официальные основания классификации
- AMD описывает Radeon RX 400 как рассчитанные на низкоуровневые API, включая Vulkan: https://ir.amd.com/news-events/press-releases/detail/692/amd-provides-sneak-peek-of-full-line-of-radeontm-rx-series-graphics-processors-at-e3
- AMD прямо указывает оптимизацию Radeon RX 500 для низкоуровневых API DirectX 12 и Vulkan: https://ir.amd.com/news-events/press-releases/detail/763/radeontm-rx-500-series-the-most-compelling-graphics-card-upgrade-yet
- AMD перечисляет Radeon RX Vega 56 и 64 и связывает архитектурные улучшения с низкоуровневыми API, включая Vulkan: https://ir.amd.com/news-events/press-releases/detail/782/amd-redefines-the-enthusiast-gaming-experience-with-radeon-rx-vega-and-radeon-packs
