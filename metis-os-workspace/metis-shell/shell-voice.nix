# Metis Core development shell — includes voice pipeline build deps (Phase 4).
#
# Usage:
#   nix-shell shell-voice.nix --run "cargo run"
#
# Requires whisper-rs and gstreamer uncommented in Cargo.toml.

{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  buildInputs = with pkgs; [
    cargo
    rustc
    pkg-config

    gtk4
    libadwaita
    adwaita-icon-theme

    # whisper-rs-sys / bindgen
    clang
    llvmPackages.libclang
    cmake
    stdenv.cc

    # GStreamer capture
    gstreamer
    gst_all_1
    gst-plugins-base
    gst-plugins-good

    nix
  ];

  shellHook = ''
    export LIBCLANG_PATH="${pkgs.llvmPackages.libclang.lib}/lib"
  '';
}
