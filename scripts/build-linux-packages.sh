#!/usr/bin/env bash
set -euo pipefail

script_directory="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
project_root="$(dirname "$script_directory")"
cargo_version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$project_root/Cargo.toml" | head -n 1)"
version="${PACKAGE_VERSION:-$cargo_version}"
output_directory="$project_root/generated-artifacts/linux"
stage_directory="$project_root/target/linux-package-staging"
rpm_top_directory="$project_root/target/rpmbuild"

if [[ -z "$cargo_version" || -z "$version" ]]; then
  printf 'Не удалось определить версию игры из Cargo.toml.\n' >&2
  exit 1
fi
if [[ -n "${PACKAGE_VERSION:-}" && ! "$version" =~ ^[0-9]+(\.[0-9]+){3}$ ]]; then
  printf 'PACKAGE_VERSION должен иметь вид r.mmyy.v.c без типа сборки.\n' >&2
  exit 1
fi
for tool in dpkg-deb rpmbuild; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    printf 'Для сборки Linux-пакетов требуется команда %s.\n' "$tool" >&2
    exit 1
  fi
done

cd "$project_root"
"$project_root/scripts/with-rust-toolchain.sh" cargo build --locked --release --bin north-seas-adventure

rm -rf -- "$output_directory" "$stage_directory" "$rpm_top_directory"
mkdir -p \
  "$output_directory" \
  "$stage_directory/usr/bin" \
  "$stage_directory/usr/libexec/north-seas-adventure" \
  "$stage_directory/usr/share/applications" \
  "$stage_directory/usr/share/icons/hicolor/scalable/apps"

install -m 0755 \
  "$project_root/target/release/north-seas-adventure" \
  "$stage_directory/usr/libexec/north-seas-adventure/north-seas-adventure"
install -m 0755 \
  "$project_root/packaging/linux/launch-game.sh" \
  "$stage_directory/usr/bin/north-seas-adventure"
install -m 0644 \
  "$project_root/packaging/linux/north-seas-adventure.desktop" \
  "$stage_directory/usr/share/applications/north-seas-adventure.desktop"
install -m 0644 \
  "$project_root/artifacts/north-seas-adventure/public/compass.svg" \
  "$stage_directory/usr/share/icons/hicolor/scalable/apps/north-seas-adventure.svg"

deb_directory="$project_root/target/linux-deb"
rm -rf -- "$deb_directory"
mkdir -p "$deb_directory/DEBIAN"
cp -a "$stage_directory/." "$deb_directory/"
architecture="$(dpkg --print-architecture)"
cat > "$deb_directory/DEBIAN/control" <<EOF
Package: north-seas-adventure
Version: $version
Section: games
Priority: optional
Architecture: $architecture
Depends: libc6, libgcc-s1, libx11-6, libvulkan1, libegl1, libgl1, libgbm1
Maintainer: NorthSea contributors <noreply@github.com>
Description: Морское приключение в раннем Средневековье
 Нативное приложение запускает игру с графикой устройства.
EOF
dpkg-deb --build --root-owner-group \
  "$deb_directory" \
  "$output_directory/north-seas-adventure_${version}_${architecture}.deb"

mkdir -p \
  "$rpm_top_directory/BUILD" \
  "$rpm_top_directory/RPMS" \
  "$rpm_top_directory/SOURCES" \
  "$rpm_top_directory/SPECS" \
  "$rpm_top_directory/SRPMS"
rpm_source_directory="$rpm_top_directory/SOURCES/north-seas-adventure-$version"
mkdir -p "$rpm_source_directory"
cp -a "$stage_directory/." "$rpm_source_directory/"
tar -C "$rpm_top_directory/SOURCES" \
  -czf "$rpm_top_directory/SOURCES/north-seas-adventure-$version.tar.gz" \
  "north-seas-adventure-$version"
sed "s/@VERSION@/$version/g" \
  "$project_root/packaging/linux/north-seas-adventure.spec.in" \
  > "$rpm_top_directory/SPECS/north-seas-adventure.spec"
rpmbuild --define "_topdir $rpm_top_directory" \
  -bb "$rpm_top_directory/SPECS/north-seas-adventure.spec"
cp "$rpm_top_directory"/RPMS/x86_64/north-seas-adventure-"$version"-1*.rpm \
  "$output_directory/"

printf 'Linux-пакеты готовы:\n'
printf '  %s\n' "$output_directory"/*
