#!/usr/bin/env nu

# Build Cap Production Version

print "========================================"
print "Building Cap Production Version"
print "========================================"
print ""

# Ensure NODE_ENV is set to production
$env.NODE_ENV = "production"

print $"✓ Environment: NODE_ENV=($env.NODE_ENV)"
print ""

# Step 1: Setup
print "Step 1: Running setup..."
pnpm cap-setup

# Step 2: Build frontend
print ""
print "Step 2: Building frontend (PRODUCTION MODE)..."
pnpm turbo build --filter @cap/desktop

# Step 3: Build Tauri with production config
print ""
print "Step 3: Building Tauri with PRODUCTION config..."

let bat_content = '@echo off
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
set NODE_ENV=production
cd apps\desktop
pnpm tauri build --config src-tauri/tauri.prod.conf.json --verbose
'

print "Creating temporary build script..."
$bat_content | save -f temp-build-prod.bat

print "Running production build with Visual Studio environment..."
cmd /c temp-build-prod.bat

print "Cleaning up..."
rm temp-build-prod.bat

print ""
print "========================================"
print "✅ Build Complete!"
print "========================================"
print ""
print "📦 Executable location:"
print "   target/release/Cap.exe"
print ""
print "📦 Installer location:"
print "   target/release/bundle/nsis/Cap_*_x64-setup.exe"
print ""
print "💡 Tips:"
print "   - This is a PRODUCTION build (optimized)"
print "   - NODE_ENV=production (frontend optimized)"
print "   - Rust compiled in release mode"
print "   - Memory usage should be minimal"
print ""
