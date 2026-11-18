# Review: AI-Generated Corporate Structure Proposal

> **Document:** "Revised Corporate Structure, Governance and Compliance Proposal"  
> **Entities:** Kharat & Co. (Parent) → AEGIS Quant Pvt Ltd + PKBoost AI Lab

---

## What This Document Is

An 11-section legal-style proposal covering:

| Section | Topic |
|---------|-------|
| 1 | Group structure — Kharat & Co. holding 100% of AEGIS Quant + PKBoost |
| 2 | Compartmentalisation — separate books, boards, personnel, compliance |
| 3 | Tax optimisation with defensible substance |
| 4 | IP ownership, assignment, and licensing framework |
| 5 | Open-sourcing policy (what can/can't be released publicly) |
| 6 | Arm's-length and transfer-pricing framework |
| 7 | Risk controls and governance safeguards |
| 8 | Crypto trading compliance (Section 115BBH) |
| 9 | Dividend and flow-through planning |
| 10 | Implementation roadmap |
| 11 | Conclusion |

---

## ✅ What It Gets Right

### 1. Substance Over Form — Nailed It
The document repeatedly emphasises that the structure must be **"genuine in substance, not merely in form"** and **"not a paper arrangement for tax efficiency alone."** This is the single most important thing to get right when the Income Tax department reviews a group structure. The AI clearly understood the risk of being seen as a sham arrangement.

### 2. Compartmentalisation is Thorough
Separate books, separate banks, separate boards, separate compliance, ring-fenced data access, RBAC on code repos — this is textbook corporate veil protection. If AEGIS blows up on a bad trade, PKBoost stays untouched (provided you **actually follow** these separation rules in practice).

### 3. Transfer Pricing Awareness
References to **Sections 92C, 92CA, 92D, 92E** (Income-tax Act) and **Section 188** (Companies Act) for related-party dealings are correct and important. The document correctly flags that inter-company charges need benchmarking, contemporaneous documentation, invoices, timesheets, and evidence of actual benefit.

### 4. Open-Source Policy is Clever
Most corporate proposals for tech companies completely ignore this. The distinction between open-source outputs, proprietary outputs, internal tools, and commercial outputs — with pre-release checks against confidential info, client commitments, and licensing obligations — is genuinely useful. This protects PKBoost from accidentally open-sourcing something that AEGIS paid a licensing fee for.

### 5. Crypto Tax Sensitivity
Correctly identifies Section 115BBH as the governing provision and flags that inter-company charges to AEGIS must be **"separately justified as a genuine business expense"** and must not appear designed to erode the taxable trading base. This is exactly what a tax officer would scrutinise.

---

## ⚠️ What It Misses or Gets Wrong

### 1. "Kharat & Co." — What Entity Type Is This?

> [!WARNING]
> The document says the parent is **"presently Kharat & Co."** but never specifies what it is — proprietorship? Partnership firm? LLP? This matters enormously.

- If it's a **partnership firm or proprietorship**, it **cannot hold shares** in a Pvt Ltd company the way the document describes. You'd need to convert it to a Pvt Ltd or LLP first.
- If it's already a **Pvt Ltd**, then why call it "Kharat & Co."? The name suggests a partnership firm.
- **Action needed:** Clarify the current legal form and decide whether to convert to Pvt Ltd (recommended) or LLP.

### 2. No Specifics on AEGIS vs. AERIS Naming

The document calls it **"AEGIS Quant Pvt Ltd"** — your project name in the earlier PDFs was **"AERIS."** Is AEGIS the final name? Note that "AEGIS" is heavily used by insurance/financial companies globally — check MCA name availability early. If the quant trading PDFs use "AERIS" as the project codename and "AEGIS" as the entity name, document that mapping.

### 3. Dividend Flow-Through is Hand-Waved

Section 9 says dividend treatment "must be reviewed" but doesn't actually state the current position:

| Flow | Current Tax Treatment (India, FY 2026–27) |
|------|------------------------------------------|
| AEGIS → Kharat & Co. (if Pvt Ltd) | Taxable as income in the hands of the parent. No DDT since 2020, but recipient pays at applicable rate. |
| PKBoost → Kharat & Co. | Same as above |
| Kharat & Co. → Pushp Kharat | Taxable as dividend income at your slab rate. TDS at 10% if > ₹5,000. |

The document should have stated this explicitly instead of deferring everything to "review."

### 4. No Discussion of Director Overlap Risk

You'll likely be a director on **all three boards.** This creates:
- **Section 184/188 risk:** Interested director in every related-party transaction
- **Mitigation:** You must formally disclose interest, abstain from voting on inter-company contracts, and ideally appoint at least one independent director per entity (even if not legally required for small Pvt Ltd)

The document completely skips this.

### 5. No FEMA / LRS Discussion

If AEGIS trades on **foreign exchanges** (Binance, Bybit, OKX — none are Indian), you're remitting funds abroad. This triggers:
- **FEMA compliance** — you can't freely wire money to foreign crypto exchanges from a company account without RBI-compliant documentation
- **LRS limits** — \$250K/person/year for individuals; companies have different rules under current account transactions
- **TCS on remittance** — 20% TCS on amounts exceeding ₹7L under LRS (refundable against tax)

The document ignores this entirely, which is a significant gap for a crypto trading firm that by definition operates on foreign venues.

### 6. No GST Analysis

Inter-company services (ML model licensing, consulting fees from PKBoost → AEGIS) are **taxable under GST at 18%** as "information technology software services" or "consulting services." This needs to be baked into the cost model. The document doesn't mention GST once.

### 7. IP Assignment Framework is Too Generic

The document says "a written IP assignment framework shall govern employee-created and contractor-created work" but doesn't specify:
- **Who owns IP created by you (Pushp)?** As founder-director of both entities, your personal IP creation needs explicit assignment to the correct entity.
- **Pre-existing IP:** Any ML models, code, or research you've already built under PKBoost MSME — how does that transfer to PKBoost Pvt Ltd? Needs a formal IP assignment deed.
- **Contractor agreements:** Indian courts require **specific** work-for-hire clauses for IP ownership to vest in the commissioning party (Copyright Act, 1957, Section 17). Generic language won't survive a dispute.

---

## 📊 Comparison: This Proposal vs. Our Earlier Plan

| Aspect | AI-Generated Proposal | Our Earlier Plan |
|--------|----------------------|------------------|
| Entity structure | ✅ Same: Holding → AEGIS + PKBoost | ✅ Same |
| Compartmentalisation detail | ✅ Stronger — more granular separation rules | ✅ Good but less detailed |
| Tax treatment specifics | ⚠️ Defers everything to "review" | ✅ States actual rates (30% crypto, 25% corp) |
| FEMA / foreign exchange compliance | ❌ Missing | ⚠️ Mentioned but not detailed |
| GST on inter-company services | ❌ Missing | ❌ Missing (we missed this too) |
| Open-source policy | ✅ Unique addition — well done | ❌ We didn't cover this |
| Director overlap / conflict of interest | ❌ Missing | ❌ We didn't cover this either |
| Crypto-specific compliance | ✅ Good — 115BBH, substance-over-form | ✅ Good — similar coverage |
| Transfer pricing framework | ✅ Strong — cites correct IT Act sections | ✅ Mentioned but less detailed |
| Implementation roadmap | ⚠️ Generic pre/post steps | ✅ More detailed phased plan |
| Cost estimates | ❌ None | ✅ Full cost breakdown |
| Trading capital requirements | ❌ None (out of scope) | ✅ Detailed |

---

## 🎯 Verdict

> [!IMPORTANT]
> **This is a solid governance document — probably 7/10 — but it reads like a template, not a tailored legal opinion.** It's the kind of output you'd get from a good AI that's read a lot of Company Law and IT Act provisions but hasn't sat with your CA and understood your specific situation.

**Use it as:** A checklist and governance framework to hand to your actual lawyer/CS for finalisation.

**Don't use it as:** A final legal document. It needs entity-specific customisation, actual pricing numbers, FEMA analysis, GST planning, and director conflict-of-interest protocols.

---

## Actionable Next Steps

1. **Clarify Kharat & Co.'s entity type** and decide on conversion path
2. **Finalise naming:** AEGIS vs AERIS — do an MCA name search
3. **Fill the FEMA gap** — talk to your CA about how AEGIS will fund foreign exchange accounts legally
4. **Add GST to the inter-company cost model** — 18% on licensing/consulting fees from PKBoost → AEGIS
5. **Draft actual IP assignment deeds** for your pre-existing work
6. **Appoint nominee/independent directors** to handle Section 188 related-party compliance
7. **Merge this governance doc with our roadmap** to create one unified implementation plan

---

*Want me to merge this proposal with our earlier roadmap + investment plan into one comprehensive master document?*
