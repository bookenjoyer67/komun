# Decision 099 - API Connection Approach

**Date:** 2026-09-28
**Review by:** 2026-12-27
**Status:** Active

**Decision:** We connect to the data API using a service account.

**Rationale:** The service account was set up by the infrastructure team. The credential is not recorded here: it is held as the environment variable `KOMUN_DATA_API_KEY`, and the infrastructure team's secret store is the source of record.

**Alternatives rejected:** Using personal credentials was rejected for security reasons.
