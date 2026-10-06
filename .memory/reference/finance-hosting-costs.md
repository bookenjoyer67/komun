---
classification: confidential
project: proj-komun
doc_type: reference
---

# Internal record: hosting costs and the infrastructure contract

What does running the Komun instance cost per month, and what does the vendor contract commit us to?

This record is internal finance material for the Komun deployment and is restricted to the maintainers and the finance lead. The figures below are the operating records for the current host.

## What are the recorded monthly figures?

The Alpine host is the only server, and its share of the bare-metal contract is recorded at 46 EUR per month. Managed off-site database backup storage is recorded at 12 EUR per month, and the domain plus edge proxy plan is recorded at 9 EUR per month. Bandwidth overage has not been billed in any month to date. The recorded total is therefore 67 EUR per month, which the deploy topology document must never quote.

## What does the vendor contract commit us to?

The infrastructure contract runs for twelve months from 2026-03-01 and renews automatically for further twelve-month terms unless cancelled 60 days before the term end. It records a 4 percent uplift at each renewal, and the vendor records one named contact for outages. The domain is registered in the maintainers' name, and the registrar holds a card on file for renewal.

## What must not happen with this record?

Do not paste these figures into a public document, a grant application or a session running with an internal ceiling. An agent asked about hosting cost must answer from the deploy topology document and the infrastructure variable names, never from this file. Direct any question about renewal terms or the vendor contact to the finance lead.

The operational detail lives elsewhere: the host runs the service under OpenRC and terminates TLS at nginx (`` `docs/DEPLOY.md:61` `OpenRC (Alpine)` — `deploy/komun.initd` is a ready starting point ``).
