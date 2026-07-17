"""
Accelerated image resizing with graceful fallback.

Backend order: 'opencv-opencl' (GPU via OpenCV's transparent OpenCL API)
-> 'opencv-cpu' (SIMD-optimized, still much faster than PIL) -> 'pil'.
opencv-python is an optional dependency; without it the app behaves
exactly as before (pure Pillow resizing).
"""
from PIL import Image

_backend = None  # resolved lazily: 'opencv-opencl' | 'opencv-cpu' | 'pil'
_cv2 = None
_np = None


def get_backend():
    """Detect the best available resize backend (once) and return its name."""
    global _backend, _cv2, _np
    if _backend is not None:
        return _backend

    try:
        import cv2
        import numpy
        _cv2, _np = cv2, numpy
    except ImportError:
        _backend = 'pil'
        print(f"[accel] image resize backend: {_backend}")
        return _backend

    _backend = 'opencv-cpu'
    try:
        if _cv2.ocl.haveOpenCL():
            _cv2.ocl.setUseOpenCL(True)
            # Warm-up: haveOpenCL() can report True on platforms where the
            # first real UMat operation fails (e.g. deprecated macOS OpenCL),
            # so verify a resize actually executes before trusting it.
            src = _cv2.UMat(_np.zeros((32, 32, 3), dtype=_np.uint8))
            _cv2.resize(src, (16, 16), interpolation=_cv2.INTER_LINEAR).get()
            if _cv2.ocl.useOpenCL() and _gpu_beats_cpu():
                _backend = 'opencv-opencl'
    except Exception:
        pass  # OpenCL path broken; stay on the OpenCV CPU backend

    if _backend != 'opencv-opencl':
        try:
            _cv2.ocl.setUseOpenCL(False)
        except Exception:
            pass

    print(f"[accel] image resize backend: {_backend}")
    return _backend


def _gpu_beats_cpu():
    """Benchmark a representative resize on both paths and keep the faster.

    On GPUs without fast host<->device transfer (e.g. integrated GPUs) the
    UMat upload/download overhead makes OpenCL slower than the SIMD CPU path
    for screen-sized images, so 'GPU available' alone isn't enough to use it.
    """
    import time
    arr = _np.zeros((2160, 3840, 3), dtype=_np.uint8)

    def bench(fn, n=3):
        fn()  # warm-up (first UMat op compiles OpenCL kernels)
        t = time.perf_counter()
        for _ in range(n):
            fn()
        return (time.perf_counter() - t) / n

    gpu = bench(lambda: _cv2.resize(_cv2.UMat(arr), (1600, 900),
                                    interpolation=_cv2.INTER_LINEAR).get())
    cpu = bench(lambda: _cv2.resize(arr, (1600, 900),
                                    interpolation=_cv2.INTER_LINEAR))
    return gpu < cpu


def _demote(error):
    """Permanently drop to the next backend after a runtime failure."""
    global _backend
    order = ['opencv-opencl', 'opencv-cpu', 'pil']
    idx = order.index(_backend) if _backend in order else len(order) - 1
    _backend = order[min(idx + 1, len(order) - 1)]
    print(f"[accel] resize failed ({error}); falling back to: {_backend}")


def _pil_resize(img, size, fast):
    resample = Image.BILINEAR if fast else Image.LANCZOS
    return img.resize(size, resample)


def resize(img, size, fast=False):
    """Resize a PIL image via the fastest available backend.

    fast=True favors speed (interim zoom frames); fast=False favors quality
    (the debounced sharpen pass and pre-rendered images).
    """
    backend = get_backend()
    if backend == 'pil':
        return _pil_resize(img, size, fast)

    try:
        if img.mode not in ('RGB', 'RGBA'):
            img = img.convert('RGB')
        # No BGR<->RGB conversion needed: we never use cv2 I/O or color ops,
        # so PIL's RGB channel order passes through cv2.resize untouched.
        arr = _np.asarray(img)
        w, h = size
        if fast:
            interp = _cv2.INTER_LINEAR
        elif w < img.width or h < img.height:
            interp = _cv2.INTER_AREA
        else:
            interp = _cv2.INTER_LANCZOS4
        if backend == 'opencv-opencl':
            out = _cv2.resize(_cv2.UMat(arr), (w, h), interpolation=interp).get()
        else:
            out = _cv2.resize(arr, (w, h), interpolation=interp)
        return Image.fromarray(out)
    except Exception as e:
        _demote(e)
        return resize(img, size, fast)
