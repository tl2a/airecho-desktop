project(
  'kopico-desktop',
  'rust',
  version: '0.2.0',
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

app_id   = 'in.tl2a.kopico-desktop'
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
  input:   [gresource_xml, data_dir / 'style.css'] + blp_to_ui,
  output:  app_id + '.gresource',
  command: [
    glib_compile_resources,
    '--sourcedir', meson.current_build_dir() / 'data',
    '--sourcedir', data_dir,   # hand-written resources (style.css)
    '--target', '@OUTPUT@',
    gresource_xml,
  ],
  install:        true,
  install_dir:    datadir / 'kopico-desktop',
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
  output:  'kopico-desktop-bin',      # placeholder; real binary is in target/
  console: true,
  command: [
    cargo, 'build',
  ] + cargo_options,
  depends: [compiled_gresource],
  env:     cargo_env,
)

# ── Install binary ────────────────────────────────────────────────────────────
install_data(
  meson.project_source_root() / 'target' / rust_target / 'kopico-desktop',
  install_dir: bindir,
  rename:      'kopico-desktop',
)

# ── Install .desktop file (required for gio::Notification on Linux) ───────────
install_data(
  'data/in.tl2a.kopico-desktop.desktop',
  install_dir: datadir / 'applications',
)

