#!/usr/bin/env nu

# Build Cap Desktop Application for Windows

print "🚀 Starting Cap Desktop build..."

# Create a temporary batch file to handle the build
let bat_content = '@echo off
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
pnpm tauri:build
'

print "📝 Creating temporary build script..."
$bat_content | save -f temp-build.bat

print "🔨 Running build with Visual Studio environment..."
cmd /c temp-build.bat

print "🧹 Cleaning up..."
rm temp-build.bat

print "✅ Build completed!"
print "📦 Executable should be at: apps/desktop/src-tauri/target/release/cap.exe"
