import subprocess
import pytest
from pathlib import Path
import tempfile
import shlex
import sys
import os

# Define paths
THIS_DIR = Path(__file__).parent
# Use cases from working_dir
CASES_DIR = Path("/usr/local/google/home/vasic/projects/ultimate-prompt/working_dir/black/tests/data/cases")
BLACK_EXE = Path(os.environ.get("BLACK_EXE", "/usr/local/google/home/vasic/projects/ultimate-prompt/working_dir/black/.venv/bin/black"))

def read_data(file_path):
    with open(file_path, "r", encoding="utf8") as f:
        lines = f.readlines()
    
    _input = []
    _output = []
    result = _input
    flags = []
    
    for line in lines:
        if not _input and line.startswith("# flags: "):
            flags = shlex.split(line[len("# flags: "):])
            if any(f.startswith("--line-ranges") for f in flags):
                _input.append(line)
            continue
        
        line = line.replace("# EMPTY LINE WITH WHITESPACE (this comment will be removed)", "")
        
        if line.rstrip() == "# output":
            result = _output
            continue
            
        result.append(line)
        
    if _input and not _output:
        _output = _input[:]
        
    return "".join(_input).strip() + "\n", "".join(_output).strip() + "\n", flags

def get_cases():
    if not CASES_DIR.exists():
        return []
    return [p.stem for p in CASES_DIR.glob("*.py")]

@pytest.mark.parametrize("case_name", get_cases())
def test_equivalence(case_name):
    file_path = CASES_DIR / f"{case_name}.py"
    input_str, expected_str, flags = read_data(file_path)
    
    # Filter out harness flags if any (e.g. --minimum-version)
    black_flags = []
    skip_test = False
    for flag in flags:
        if flag.startswith("--minimum-version="):
            version = flag.split("=")[1]
            major, minor = map(int, version.split("."))
            if sys.version_info < (major, minor):
                skip_test = True
        elif flag == "--no-preview-line-length-1":
            pass # Ignore harness flag
        elif flag.startswith("--line-ranges="):
            black_flags.append(flag)
        else:
            black_flags.append(flag)
            
    if skip_test:
        pytest.skip(f"Requires Python >= {version}")
        
    with tempfile.TemporaryDirectory() as temp_dir:
        temp_file = Path(temp_dir) / "input.py"
        temp_file.write_text(input_str, encoding="utf-8")
        
        # Run black
        cmd = [str(BLACK_EXE), *black_flags, str(temp_file)]
        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True
        )
        
        # Black might return non-zero if it failed to parse invalid input,
        # but some tests might expect that?
        # For simple equivalence, we expect success.
        assert result.returncode == 0, f"Black failed with stderr: {result.stderr}"
        
        actual_str = temp_file.read_text(encoding="utf-8")
        
        assert actual_str == expected_str
