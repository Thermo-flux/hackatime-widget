# Dot-source this (. .\env.ps1) to keep the whole toolchain and build output on D:
$env:TEMP = 'D:\dev\tmp'; $env:TMP = 'D:\dev\tmp'
$env:RUSTUP_HOME = 'D:\dev\rustup'
$env:CARGO_HOME = 'D:\dev\cargo'
$env:CARGO_TARGET_DIR = 'D:\dev\target\hackatime-widget'
$env:Path = "D:\dev\cargo\bin;D:\dev\mingw64\bin;$env:Path"
