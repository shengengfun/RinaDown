cmake_minimum_required(VERSION 3.10)
set(PROJECT_NAME "tray_manager")
project(${PROJECT_NAME} LANGUAGES CXX)

# This value is used when generating builds using this plugin, so it must
# not be changed
set(PLUGIN_NAME "tray_manager_plugin")

find_package(PkgConfig REQUIRED)

add_library(${PLUGIN_NAME} SHARED
  "tray_manager_plugin.cc"
)
apply_standard_settings(${PLUGIN_NAME})
set_target_properties(${PLUGIN_NAME} PROPERTIES
  CXX_VISIBILITY_PRESET hidden)
target_compile_definitions(${PLUGIN_NAME} PRIVATE FLUTTER_PLUGIN_IMPL)
target_include_directories(${PLUGIN_NAME} INTERFACE
  "${CMAKE_CURRENT_SOURCE_DIR}/include")
target_link_libraries(${PLUGIN_NAME} PRIVATE flutter)
target_link_libraries(${PLUGIN_NAME} PRIVATE PkgConfig::GTK)

# SNI implementation uses GDBus (gio-2.0) and GdkPixbuf for icon loading.
# Both are transitive dependencies of GTK3 but we list them explicitly for
# clarity and to ensure correct linker flags on all distros.
pkg_check_modules(GIO      REQUIRED IMPORTED_TARGET gio-2.0)
pkg_check_modules(GDKPIXBUF REQUIRED IMPORTED_TARGET gdk-pixbuf-2.0)
target_link_libraries(${PLUGIN_NAME} PRIVATE PkgConfig::GIO PkgConfig::GDKPIXBUF)

# List of absolute paths to libraries that should be bundled with the plugin
set(tray_manager_bundled_libraries
  ""
  PARENT_SCOPE
)
