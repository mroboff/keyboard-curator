# A development shell for building and running Keyboard Curator on NixOS
# and other Nix systems: the Rust toolchain comes from rustup as usual;
# this supplies the system libraries the GUI, audio and USB crates link
# against, and puts the ones loaded at run time on the library path.
#
#   nix-shell --run "cargo run -p kc-app"
{ pkgs ? import <nixpkgs> { } }:
let
  runtimeLibs = with pkgs; [
    wayland
    libxkbcommon
    vulkan-loader
    (pkgs.libx11 or pkgs.xorg.libX11)
    (pkgs.libxcb or pkgs.xorg.libxcb)
    (pkgs.libxcursor or pkgs.xorg.libXcursor)
    (pkgs.libxi or pkgs.xorg.libXi)
    fontconfig
    freetype
    alsa-lib
    udev
    openssl
  ];
in
pkgs.mkShell {
  packages = with pkgs; [
    pkg-config
    cmake
    clang
    llvmPackages.libclang
  ];
  buildInputs = runtimeLibs;
  LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeLibs;
}
