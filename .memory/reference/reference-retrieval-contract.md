---
classification: internal
project: proj-komun
doc_type: reference
---

# Reference: the retrieval tool contract

What does a `retrieve` call take, what does it return, and when does it fall back?

The corpus is this directory, read at startup with each document's front matter supplying its metadata (`mcp/retrieval/server.py:35` `REFERENCE_DIR = os.getenv("RETRIEVAL_REFERENCE_DIR", str(Path(MEMORY_DIR) / "reference"))`).

## What does a query carry?

A query carries a query string, a required project id, an optional result count, a ceiling and optional metadata filters (`mcp/retrieval/server.py:450` `top_k: int = 3,`). The default ceiling is internal (`mcp/retrieval/server.py:41` `DEFAULT_CEILING = "internal"`). The four levels are ordered least to most sensitive (`mcp/retrieval/server.py:50` `CLASSIFICATION_ORDER = ("public", "internal", "confidential", "secret")`).

## When must the keyword fallback serve a query?

A query with no vector match above the confidence threshold falls back to a keyword search (`mcp/retrieval/server.py:8` `finds no vector match above the confidence threshold falls back to a rank_bm25 keyword search`). The threshold is 0.65 unless an environment variable overrides it (`mcp/retrieval/server.py:40` `SIMILARITY_THRESHOLD = float(os.getenv("RETRIEVAL_SIMILARITY_THRESHOLD", "0.65"))`). A fallback result reports its method and carries no score (`mcp/retrieval/server.py:480` `return [chunk.as_result(None, "keyword") for chunk in fallback]`).

## What is the ceiling rule?

The ceiling filters the eligible chunk set before either search runs, so no document above it can be returned (`mcp/retrieval/server.py:466` `eligible = INDEX.eligible_ids(project_id, ceiling_rank, filters)`). Absence is therefore the pass condition for a ceiling query, not a hit (`docs/integration-test.txt:133` `absent from both.`). A vector hit keeps only scores at or above the threshold (`mcp/retrieval/server.py:376` `if similarity < SIMILARITY_THRESHOLD:`).
