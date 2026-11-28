// Package engine locates and builds the Rust engine that backs a generated
// project. Generated apps do not vendor the engine source; `mre build` finds
// the checkout next to the app (or via MRE_ENGINE) and produces the native
// library from it.
package engine

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
)

// Find walks up from start looking for an MRE workspace checkout.
func Find(start string) (string, error) {
	if explicit := os.Getenv("MRE_ENGINE"); explicit != "" {
		if ok := isWorkspace(explicit); ok {
			return filepath.Abs(explicit)
		}
		return "", fmt.Errorf("MRE_ENGINE=%s is not an MRE workspace", explicit)
	}
	dir, err := filepath.Abs(start)
	if err != nil {
		return "", err
	}
	for {
		if isWorkspace(dir) {
			return dir, nil
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			return "", fmt.Errorf("no MRE engine found; set MRE_ENGINE to the engine checkout")
		}
		dir = parent
	}
}

func isWorkspace(dir string) bool {
	info, err := os.Stat(filepath.Join(dir, "crates", "mre-android", "Cargo.toml"))
	return err == nil && !info.IsDir()
}

// AndroidNDK returns the configured NDK path.
func AndroidNDK() (string, error) {
	for _, key := range []string{"ANDROID_NDK_HOME", "NDK"} {
		if value := os.Getenv(key); value != "" {
			return value, nil
		}
	}
	return "", fmt.Errorf("ANDROID_NDK_HOME is not set (install Android NDK r27c)")
}

// BuildAndroid cross-compiles libmain.so and assembles the APK.
func BuildAndroid(engineDir, root string, prod bool) (string, error) {
	ndk, err := AndroidNDK()
	if err != nil {
		return "", err
	}
	toolchain := filepath.Join(ndk, "toolchains", "llvm", "prebuilt", "linux-x86_64", "bin")
	if _, err := os.Stat(filepath.Join(toolchain, "aarch64-linux-android24-clang++")); err != nil {
		return "", fmt.Errorf("NDK at %s is missing the aarch64 clang toolchain", ndk)
	}

	cargo, err := exec.LookPath("cargo")
	if err != nil {
		return "", fmt.Errorf("cargo is required to build Android: %w", err)
	}
	env := append(os.Environ(),
		"CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="+filepath.Join(toolchain, "aarch64-linux-android24-clang++"),
		"CC_aarch64_linux_android="+filepath.Join(toolchain, "aarch64-linux-android24-clang"),
		"CXX_aarch64_linux_android="+filepath.Join(toolchain, "aarch64-linux-android24-clang++"),
		"AR_aarch64_linux_android="+filepath.Join(toolchain, "llvm-ar"),
		"RUSTFLAGS=-C link-arg=-llog",
	)
	build := exec.Command(cargo, "build", "--release", "--target", "aarch64-linux-android", "-p", "mre-android")
	build.Dir = engineDir
	build.Env = env
	build.Stdout, build.Stderr = os.Stdout, os.Stderr
	if err := build.Run(); err != nil {
		return "", err
	}

	libDir := filepath.Join(root, "android", "app", "src", "main", "jniLibs", "arm64-v8a")
	if err := os.MkdirAll(libDir, 0o755); err != nil {
		return "", err
	}
	source := filepath.Join(engineDir, "target", "aarch64-linux-android", "release", "libmain.so")
	data, err := os.ReadFile(source)
	if err != nil {
		return "", err
	}
	if err := os.WriteFile(filepath.Join(libDir, "libmain.so"), data, 0o644); err != nil {
		return "", err
	}

	assets := filepath.Join(root, "android", "app", "src", "main", "assets")
	if err := os.MkdirAll(filepath.Join(assets, "framework"), 0o755); err != nil {
		return "", err
	}
	entry, err := os.ReadFile(filepath.Join(root, "src", "main.lua"))
	if err != nil {
		return "", err
	}
	if err := os.WriteFile(filepath.Join(assets, "main.lua"), entry, 0o644); err != nil {
		return "", err
	}
	framework := filepath.Join(root, "framework")
	files, err := os.ReadDir(framework)
	if err == nil {
		for _, file := range files {
			if file.IsDir() || filepath.Ext(file.Name()) != ".lua" {
				continue
			}
			data, err := os.ReadFile(filepath.Join(framework, file.Name()))
			if err != nil {
				return "", err
			}
			if err := os.WriteFile(filepath.Join(assets, "framework", file.Name()), data, 0o644); err != nil {
				return "", err
			}
		}
	}

	gradle, err := exec.LookPath("gradle")
	if err != nil {
		return "", fmt.Errorf("gradle is required to assemble the APK: %w", err)
	}
	task := "assembleDevDebug"
	apk := filepath.Join(root, "android", "app", "build", "outputs", "apk", "dev", "debug", "app-dev-debug.apk")
	if prod {
		task = "assembleProdRelease"
		apk = filepath.Join(root, "android", "app", "build", "outputs", "apk", "prod", "release", "app-prod-release-unsigned.apk")
	}
	assemble := exec.Command(gradle, task, "--no-daemon")
	assemble.Dir = filepath.Join(root, "android")
	assemble.Stdout, assemble.Stderr = os.Stdout, os.Stderr
	if err := assemble.Run(); err != nil {
		return "", err
	}
	return apk, nil
}
