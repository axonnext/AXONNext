import sys
import os

# Add local path for testing
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "lib")))

from axon2.v2026 import Node, dumps2026
from axon2.stream import EnvelopeStream

def test_stream_envelope():
    # Simulate a stream of objects
    stream = [
        Node("StreamStart"),
        {"key": "value1"},
        {"key": "value2"},
        Node("StreamEnd", attributes={"count": 2})
    ]
    
    # Pass through validator
    validated = list(EnvelopeStream(stream))
    assert len(validated) == 2, "Stream validator failed"
    print("[OK] Stream Validation Passed")

def test_scientific_arrays():
    try:
        from axon2.scientific import from_numpy, to_numpy
        import numpy as np
    except ImportError:
        print("[SKIP] NumPy not installed, skipping array test")
        return
        
    arr = np.array([[1.0, 2.0], [3.0, 4.0]], dtype=np.float32)
    node = from_numpy(arr)
    
    print("[INFO] Encoded Array Node:", dumps2026(node))
    
    arr_back = to_numpy(node)
    assert np.allclose(arr, arr_back), "Array roundtrip failed"
    print("[OK] Scientific Arrays Passed")

if __name__ == "__main__":
    test_stream_envelope()
    test_scientific_arrays()
    print("All feature tests ran successfully.")
