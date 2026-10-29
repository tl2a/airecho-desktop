project(
  'airecho',
  'rust',
  version: '0.4.2',
  license: 'GPL-3.0-or-later',
  meson_version: '>= 1.0',
)

# ── Dependencies ──────────────────────────────────────────────────────────────
dependency('gtk4',       version: '>= 4.14')
dependency('libadwaita-1', version: '>= 1.6')

# ── Tools ─────────────────────────────────────────────────────────────────────
fs = import('fs')
blueprint_compiler = find_program('blueprint-compiler', required: true)
glib_compile_resources = find_program('glib-compile-resources', required: true)
cargo = find_program('cargo', required: true)

# ── Directories ───────────────────────────────────────────────────────────────
prefix  = get_option('prefix')
bindir  = prefix / get_option('bindir')
datadir = prefix / get_option('datadir')

app_id   = 'in.tl2a.airEcho'
data_dir = meson.project_source_root() / 'data'

# ── Blueprint → GtkBuilder XML ────────────────────────────────────────────────
blueprints = files(
  'data/ui/window.blp',
)


blp_to_ui = []
foreach blp : blueprints
  blp_name = fs.stem(blp)
  ui_file = custom_target(
    blp_name + '_ui',
    input:   blp,
    output:  blp_name + '.ui',
    command: [
      blueprint_compiler,
      'compile',
      '--output', '@OUTPUT@',
      '@INPUT@',
    ],
  )
  blp_to_ui += ui_file
endforeach

# ── GtkBuilder XML → GResource ────────────────────────────────────────────────
gresource_xml = data_dir / app_id + '.gresource.xml'

compiled_gresource = custom_target(
  'gresource',
  input:   [
    gresource_xml,
    data_dir / 'style.css',
    data_dir / 'icons/in.tl2a.airEcho.svg',
    data_dir / 'macos/index.theme',
    data_dir / 'macos/about-icon.png',
    data_dir / 'icons/in.tl2a.airEcho-status-icon-active.svg',
    data_dir / 'icons/in.tl2a.airEcho-status-icon-inactive.svg',
  ] + blp_to_ui,
  output:  app_id + '.gresource',
  command: [
    glib_compile_resources,
    '--sourcedir', meson.current_build_dir(),
    '--sourcedir', data_dir,   # hand-written resources (style.css)
    '--target', '@OUTPUT@',
    gresource_xml,
  ],
  install:        true,
  install_dir:    datadir / 'airecho',
)

# ── Cargo build ───────────────────────────────────────────────────────────────
cargo_options = ['--manifest-path', meson.project_source_root() / 'Cargo.toml']

if get_option('buildtype') == 'release'
  cargo_options += '--release'
  rust_target = 'release'
else
  rust_target = 'debug'
endif

cargo_env = environment()
cargo_env.set('MESON_BUILD_ROOT', meson.current_build_dir())

custom_target(
  'cargo-build',
  output:  'airecho-bin',      # placeholder; real binary is in target/
  build_by_default: true,
  console: true,
  command: [
    cargo, 'build',
  ] + cargo_options,
  depends: [compiled_gresource],
  env:     cargo_env,
)

# ── Install binary ────────────────────────────────────────────────────────────
install_data(
  meson.project_source_root() / 'target' / rust_target / 'airecho',
  install_dir: bindir,
  rename:      'airecho',
)

# ── Install .desktop file (required for gio::Notification on Linux) ───────────
install_data(
  'data/in.tl2a.airEcho.desktop',
  install_dir: datadir / 'applications',
)

install_data(
  'data/icons/in.tl2a.airEcho.svg',
  install_dir: datadir / 'icons' / 'hicolor' / 'scalable' / 'apps',
)
