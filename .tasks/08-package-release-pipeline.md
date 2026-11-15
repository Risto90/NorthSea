---
id: package-install-validation
title: Собирать, проверять и публиковать релизы North Sea
relationship: blocked_by|#21
relationship: blocked_by|android-update-compatibility
relationship: blocked_by|android-startup-crash
subtask: android-installed-launch|01. Проверить APK в GitHub Actions|В GitHub Actions собрать кандидатный APK, установить его в официальном Android Emulator с ARM64-образом и проверить запуск установленного приложения.
subtask: deb-installed-launch|02. Проверить DEB в GitHub Actions|В GitHub Actions собрать DEB, установить его штатным пакетным менеджером и проверить запуск приложения из установленного пакета.
subtask: rpm-installed-launch|03. Проверить RPM в GitHub Actions|В GitHub Actions собрать RPM, установить его штатным пакетным менеджером и проверить запуск приложения из установленного пакета.
subtask: source-archives|04. Проверить исходные архивы|В GitHub Actions получить ZIP и tar.gz для проверяемого коммита, собрать из каждого пакеты и проверить их установку и запуск.
subtask: fix-package-failures|05. Исправить ошибки перед публикацией|Исправить все ошибки сборки, установки и запуска, обнаруженные проверками выше, затем повторить затронутые проверки до успешного результата.|32
subtask_remove_relationship: fix-package-failures|blocked_by|#27
subtask_remove_relationship: fix-package-failures|sub_issue|#34
---

## Что и зачем
GitHub Actions собирает и публикует Android-, Linux- и веб-версии. Выпуск должен проверять именно пакеты, созданные этим запуском Actions, а не локальные копии или результаты предыдущих сборок.

## Готовый результат
- Workflow запускается вручную, а не при каждом обычном коммите.
- Пакеты APK, DEB и RPM устанавливаются и запускают приложение в проверочных средах GitHub Actions; тестовая логика остаётся на Rust.
- Исходные архивы ZIP и tar.gz проверяемого коммита независимо собираются и тестируются в GitHub Actions.
- Ошибка любого теста останавливает этот запуск: GitHub Release и Pages не публикуются.
- После исправления сбоев все проверки повторно проходят; публикация Release и Pages происходит только после их успеха.
- Все платформы используют одну базовую версию с различием только по типу сборки; неудачная сборка или публикация не увеличивает номер успешной версии.
- Веб-сборка публикуется через Pages; пакеты распространяются через GitHub Release, а не через раздел Packages.

## Вне объёма
- Публикация непроверенных пакетов или запуск релиза от обычного коммита.
