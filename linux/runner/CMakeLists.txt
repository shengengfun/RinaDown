cmake_minimum_required(VERSION 3.13)
project(runner LANGUAGES CXX C)

# Compile GResource (embeds app icon into the binary)
find_program(GLIB_COMPILE_RESOURCES glib-compile-resources REQUIRED)
get_filename_component(FLUTTER_PROJECT_ROOT "${CMAKE_SOURCE_DIR}/.." ABSOLUTE)
set(GRESOURCE_XML
  "${CMAKE_CURRENT_SOURCE_DIR}/resources/com.fluxdown.app.gresource.xml")
set(GRESOURCE_C "${CMAKE_CURRENT_BINARY_DIR}/resources.c")
add_custom_command(
  OUTPUT ${GRESOURCE_C}
  COMMAND ${GLIB_COMPILE_RESOURCES}
    --target=${GRESOURCE_C}
    --generate-source
    --sourcedir=${FLUTTER_PROJECT_ROOT}
    ${GRESOURCE_XML}
  DEPENDS
    ${GRESOURCE_XML}
    ${FLUTTER_PROJECT_ROOT}/assets/logo/fluxdown_logo.png
    ${FLUTTER_PROJECT_ROOT}/assets/logo/fluxdown_logo.svg
)

# Define the application target. To change its name, change BINARY_NAME in the
# top-level CMakeLists.txt, not the value here, or `flutter run` will no longer
# work.
#
# Any new source files that you add to the application should be added here.
add_executable(${BINARY_NAME}
  "main.cc"
  "my_application.cc"
  "floating_ball_window.cc"
  "popup_window_host.cc"
  "${GRESOURCE_C}"
  "${FLUTTER_MANAGED_DIR}/generated_plugin_registrant.cc"
)

# Apply the standard set of build settings. This can be removed for applications
# that need different build settings.
apply_standard_settings(${BINARY_NAME})

# Add preprocessor definitions for the application ID.
add_definitions(-DAPPLICATION_ID="${APPLICATION_ID}")

# Add dependency libraries. Add any application-specific dependencies here.
target_link_libraries(${BINARY_NAME} PRIVATE flutter)
target_link_libraries(${BINARY_NAME} PRIVATE PkgConfig::GTK)

target_include_directories(${BINARY_NAME} PRIVATE "${CMAKE_SOURCE_DIR}")
