from __future__ import annotations
import logging
import os
import shutil
import subprocess
from dataclasses import dataclass

log = logging.getLogger(__name__)


@dataclass
class GpuInfo:
    vendor: str      # 'nvidia' | 'amd' | 'intel' | 'unknown'
    api: str         # 'cuda' | 'vulkan' | 'none'
    vram_mb: int     # 0 if unknown


def probe_gpu() -> GpuInfo | None:
    """Lightweight GPU detection using sysfs, nvidia-smi, and vulkaninfo."""
    # 1. Try nvidia-smi first (fast, unambiguous)
    if shutil.which("nvidia-smi"):
        try:
            out = subprocess.check_output(
                ["nvidia-smi", "--query-gpu=name,memory.total", "--format=csv,noheader,nounits"],
                timeout=5,
                stderr=subprocess.DEVNULL,
            ).decode().strip()
            if out:
                parts = out.split(",")
                vram = int(parts[-1].strip()) if len(parts) >= 2 else 0
                api = "cuda" if _cuda_libs_present() else "vulkan"
                return GpuInfo(vendor="nvidia", api=api, vram_mb=vram)
        except Exception:
            pass

    # 2. Try sysfs DRM vendor IDs
    vendor = _sysfs_gpu_vendor()
    if vendor in ("amd", "intel"):
        vram = _sysfs_vram_mb()
        api = "vulkan" if _vulkan_available() else "none"
        return GpuInfo(vendor=vendor, api=api, vram_mb=vram)

    # 3. Try vulkaninfo JSON (covers all vendors including NVIDIA with Nouveau)
    vulkan_vendor = _vulkan_gpu_vendor()
    if vulkan_vendor:
        vram = _vulkan_vram_mb()
        return GpuInfo(vendor=vulkan_vendor, api="vulkan", vram_mb=vram)

    return None


def _sysfs_gpu_vendor() -> str | None:
    """Read /sys/class/drm/card*/device/vendor to detect AMD/Intel GPU."""
    drm_root = "/sys/class/drm"
    if not os.path.isdir(drm_root):
        return None
    for card in sorted(os.listdir(drm_root)):
        vendor_file = os.path.join(drm_root, card, "device", "vendor")
        try:
            with open(vendor_file) as f:
                vid = f.read().strip().lower()
            if vid == "0x1002":
                return "amd"
            if vid == "0x8086":
                return "intel"
            if vid == "0x10de":
                return "nvidia"
        except OSError:
            continue
    return None


def _sysfs_vram_mb() -> int:
    drm_root = "/sys/class/drm"
    if not os.path.isdir(drm_root):
        return 0
    for card in sorted(os.listdir(drm_root)):
        mem_file = os.path.join(drm_root, card, "device", "mem_info_vram_total")
        try:
            with open(mem_file) as f:
                return int(f.read().strip()) // (1024 * 1024)
        except (OSError, ValueError):
            continue
    return 0


def _vulkan_gpu_vendor() -> str | None:
    if not shutil.which("vulkaninfo"):
        return None
    try:
        out = subprocess.check_output(
            ["vulkaninfo", "--json"],
            timeout=10,
            stderr=subprocess.DEVNULL,
        ).decode(errors="replace")
        import json
        data = json.loads(out)
        devices = data.get("capabilities", {}).get("device", [])
        if not devices:
            # Alternative structure
            devices = data.get("VkPhysicalDeviceProperties", [])
        for dev in devices:
            props = dev.get("properties", dev)
            vid = props.get("vendorID", 0)
            if vid == 0x10DE:
                return "nvidia"
            if vid == 0x1002:
                return "amd"
            if vid == 0x8086:
                return "intel"
    except Exception:
        pass
    return None


def _vulkan_vram_mb() -> int:
    if not shutil.which("vulkaninfo"):
        return 0
    try:
        out = subprocess.check_output(
            ["vulkaninfo", "--json"],
            timeout=10,
            stderr=subprocess.DEVNULL,
        ).decode(errors="replace")
        import json
        data = json.loads(out)
        # Try to find heap size from memory properties
        for dev in data.get("capabilities", {}).get("device", []):
            mem = dev.get("memoryProperties", {})
            heaps = mem.get("memoryHeaps", [])
            for heap in heaps:
                flags = heap.get("flags", 0)
                if flags & 1:  # VK_MEMORY_HEAP_DEVICE_LOCAL_BIT
                    size = heap.get("size", 0)
                    if size > 0:
                        return size // (1024 * 1024)
    except Exception:
        pass
    return 0


def _cuda_available() -> bool:
    return shutil.which("nvidia-smi") is not None and _cuda_libs_present()


def _cuda_libs_present() -> bool:
    """Check for CUDA libraries without importing torch or ctranslate2."""
    import ctypes
    for libname in ("libcuda.so.1", "libcuda.so"):
        try:
            ctypes.CDLL(libname)
            return True
        except OSError:
            pass
    return False


def _vulkan_available() -> bool:
    return shutil.which("vulkaninfo") is not None


def select_backend(config) -> object:
    """
    Return an instantiated (but not yet loaded) TranscriptionBackend based on
    config.get('backend_engine') and available hardware.
    """
    from .faster_whisper_backend import FasterWhisperBackend
    from .whisper_cpp_backend import WhisperCppBackend
    from .moonshine_backend import MoonshineBackend

    engine = config.get("backend_engine", "auto")

    if engine == "moonshine":
        b = MoonshineBackend()
        if b.is_available:
            lang = config.get("moonshine_language", "en") or "en"
            b.configure_language(lang)
            log.info("Backend: forced to moonshine")
            return b
        log.warning(
            "Backend 'moonshine' selected but moonshine-voice is not installed. "
            "Install with: pip install moonshine-voice  "
            "Falling back to auto-detection."
        )

    if engine == "faster-whisper":
        log.info("Backend: forced to faster-whisper")
        return FasterWhisperBackend()

    if engine == "whisper-cpp":
        log.info("Backend: forced to whisper-cpp")
        cpp_binary = config.get("whisper_cpp_binary", "whisper-cli")
        cpp_model_dir = config.get("whisper_cpp_model_dir") or os.path.join(
            os.path.expanduser("~"), ".local", "share", "voxctl", "models"
        )
        backend = WhisperCppBackend(binary_path=cpp_binary, model_dir=cpp_model_dir)
        if config.get("whisper_cpp_threads"):
            backend.configure_threads(config.get("whisper_cpp_threads"))
        return backend

    # ── Auto-detection ─────────────────────────────────────────────────────
    # Moonshine is checked first: when installed it is faster and more accurate
    # than Whisper for English on any hardware (CPU, NVIDIA, AMD, Intel).
    moonshine = MoonshineBackend()
    if moonshine.is_available:
        lang = config.get("moonshine_language", "en") or "en"
        moonshine.configure_language(lang)
        log.info("Backend auto: moonshine-voice available → moonshine")
        return moonshine

    gpu = probe_gpu()
    log.info(f"GPU probe result: {gpu}")

    cpp_binary = config.get("whisper_cpp_binary", "whisper-cli")
    cpp_model_dir = config.get("whisper_cpp_model_dir") or os.path.join(
        os.path.expanduser("~"), ".local", "share", "voxctl", "models"
    )

    def make_cpp():
        b = WhisperCppBackend(binary_path=cpp_binary, model_dir=cpp_model_dir)
        if config.get("whisper_cpp_threads"):
            b.configure_threads(config.get("whisper_cpp_threads"))
        return b

    if gpu is None:
        log.info("Backend auto: no GPU detected → faster-whisper CPU")
        return FasterWhisperBackend()

    if gpu.vendor == "nvidia" and gpu.api == "cuda":
        log.info("Backend auto: NVIDIA + CUDA → faster-whisper")
        return FasterWhisperBackend()

    if gpu.vendor in ("amd", "intel") and gpu.api == "vulkan":
        cpp = make_cpp()
        if cpp.is_available:
            log.info(f"Backend auto: {gpu.vendor.upper()} + Vulkan → whisper-cpp")
            return cpp
        log.warning("Backend auto: AMD/Intel GPU found but whisper-cli not available → faster-whisper CPU")
        return FasterWhisperBackend()

    if gpu.vendor == "nvidia" and gpu.api == "vulkan":
        # NVIDIA without CUDA (Nouveau driver)
        cpp = make_cpp()
        if cpp.is_available and _vulkan_available():
            log.info("Backend auto: NVIDIA + Vulkan (no CUDA) → whisper-cpp")
            return cpp

    log.info("Backend auto: safe fallback → faster-whisper")
    return FasterWhisperBackend()


def auto_compute_type(backend_name: str, gpu: GpuInfo | None) -> str:
    """Return a sensible compute_type string given backend and detected hardware."""
    if backend_name == "faster-whisper":
        if gpu and gpu.vendor == "nvidia":
            return "float16" if gpu.vram_mb >= 6144 else "int8"
        return "int8"

    # whisper-cpp
    if gpu and gpu.vendor in ("amd", "intel"):
        return "Q8_0" if gpu.vram_mb >= 8192 else "Q5_K_M"
    return "Q5_K_M"
