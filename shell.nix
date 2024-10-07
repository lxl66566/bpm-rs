{
  pkgs ? import <nixpkgs> { },
}:

pkgs.mkShell {
  buildInputs = with pkgs; [
    git
    libarchive.dev
    openssl.dev
  ];

  shellHook = ''
    export PKG_CONFIG_ALLOW_SYSTEM_LIBS=1 
    export PKG_CONFIG_ALLOW_SYSTEM_CFLAGS=1
    export PKG_CONFIG_PATH=${pkgs.libarchive.dev}/lib/pkgconfig:${pkgs.openssl.dev}/lib/pkgconfig
  '';
}
