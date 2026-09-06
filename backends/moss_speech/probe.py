"""Check the isolated runtime without loading weights or using the GPU."""
import importlib
import json

result = {"imports": {}, "notices": [], "hints": []}
for name in ("torch", "transformers", "soundfile", "librosa", "einops", "accelerate", "scipy"):
    try:
        module = importlib.import_module(name)
        result["imports"][name] = True
        if name == "torch":
            result.update(torch=module.__version__, torch_cuda=module.version.cuda,
                          cuda_available=module.cuda.is_available())
            result["imports"]["torch_pin"] = module.__version__ == "2.9.1+cu128"
        if name == "transformers":
            result["imports"]["transformers_pin"] = module.__version__ == "5.0.0"
    except Exception as error:
        result["imports"][name] = False
        result["hints"].append(str(error))
print(json.dumps(result))
