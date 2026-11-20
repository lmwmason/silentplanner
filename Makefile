.PHONY: all download validate preprocess train evaluate analyze simulate from-processed extended cuda cuda-scale followup test lint package

PYTHON ?= python
export MPLBACKEND = Agg
export OPENBLAS_NUM_THREADS = 1
export OMP_NUM_THREADS = 1

all: simulate

download:
	$(PYTHON) scripts/download_datasets.py

validate: download
	$(PYTHON) scripts/validate_datasets.py

preprocess: validate
	$(PYTHON) scripts/preprocess.py

train: preprocess
	$(PYTHON) scripts/train.py

evaluate: train
	$(PYTHON) scripts/evaluate.py

analyze: evaluate
	$(PYTHON) scripts/analyze_results.py

simulate: analyze
	$(PYTHON) scripts/simulate.py

from-processed:
	$(PYTHON) scripts/train.py
	$(PYTHON) scripts/evaluate.py
	$(PYTHON) scripts/analyze_results.py
	$(PYTHON) scripts/simulate.py

extended:
	$(PYTHON) scripts/compare_extended.py

cuda:
	$(PYTHON) scripts/benchmark_cuda.py

cuda-scale:
	$(PYTHON) scripts/benchmark_cuda.py --scale

followup:
	$(PYTHON) scripts/compare_attenuation.py
	$(PYTHON) scripts/prepare_field_demo.py

test:
	$(PYTHON) -m pytest

lint:
	$(PYTHON) -m ruff check .
	$(PYTHON) -m ruff format --check .

package: lint test
	$(PYTHON) scripts/package_release.py
