# Graphene Network Branding Guide

## Core Brand Attributes

### Technical Foundation

The name "Graphene" references the carbon lattice material known for being:
- **Lightweight** - one atom thick, yet incredibly strong
- **Strong** - 200x stronger than steel
- **Efficient** - excellent conductor
- **Cutting-edge** - modern material science

This translates to product attributes:
- 1-5MB unikernels vs gigabyte containers
- Hardware-level isolation via MicroVMs
- Sub-second cold starts
- Modern, minimal architecture

### Key Messages

| Theme | Expression | Evidence |
|-------|------------|----------|
| **Minimal/Lightweight** | "Only what you need, nothing you don't" | 1-5MB unikernels vs GB containers |
| **Secure by Design** | "No shell. No exploits. No surprises." | No runtime installs, allowlist-only egress |
| **Fast/Instant** | "Execute now, settle later" | Sub-second cold starts, off-chain execution |
| **Developer-First** | "Ship functions, not infrastructure" | SDKs, clear docs, Lambda migration path |

---

## Visual Identity

### Design Direction: Carbon Precision

The visual identity draws from graphene's material properties—engineered at the atomic level, minimal yet incredibly strong. The aesthetic is clean, technical, and precise without being cold or inaccessible.

**Vibe:** Engineered material science. Laboratory precision. Modern infrastructure.

### Color Palette

**Carbon Scale (Primary)**
- Carbon Black (`#1A1A1A`) - primary backgrounds, emphasis
- Graphite (`#2D2D2D`) - secondary backgrounds, cards
- Charcoal (`#4A4A4A`) - borders, dividers
- Ash (`#6B6B6B`) - muted text, disabled states
- Silver (`#A3A3A3`) - secondary text
- Platinum (`#E5E5E5`) - light backgrounds, light mode

**Accent**
- Electric Blue (`#0EA5E9`) - primary accent, CTAs, links, interactive elements
- Use sparingly for maximum impact
- Single accent color maintains precision aesthetic

**Semantic**
- Success (`#22C55E`) - confirmations, security indicators, safe states
- Warning (`#F59E0B`) - cautions, important notices
- Error (`#EF4444`) - errors, destructive actions
- Info (`#3B82F6`) - informational, neutral highlights

### Typography

**Primary:** Inter or similar geometric sans-serif
- Clean, highly legible
- Works well at all sizes
- Technical without being sterile

**Monospace:** JetBrains Mono or similar
- For code examples, technical specs
- Clear distinction between similar characters
- Ligatures optional

### Visual Motifs

**Hexagonal Lattice**
- Core visual element derived from graphene's molecular structure
- Use subtly in backgrounds, patterns, loading states
- Can be abstract/partial—doesn't need to be literal
- Suggests interconnection, strength, and atomic precision

**Minimal Geometry**
- Clean lines, precise angles
- Geometric shapes over organic forms
- Avoid hand-drawn, playful, or decorative elements
- Reflects the engineered, deterministic nature of the platform

**Negative Space**
- Embrace whitespace (or dark space)
- Let elements breathe
- Density suggests complexity; we're minimal

### Logo Guidelines

**Construction**
- Based on hexagonal/carbon lattice geometry
- Clean, geometric, precise construction
- Single stroke weight or minimal variation
- Works in monochrome (required) and with accent color (optional)

**Usage**
- Minimum clear space: height of logo on all sides
- Minimum size: 24px height for digital, 10mm for print
- Always use provided assets; never recreate or modify

**Acceptable Variations**
- Full color on dark background (primary)
- Monochrome white on dark background
- Monochrome black on light background
- With or without wordmark

**Avoid**
- Gradients or effects
- Rotation or distortion
- Unapproved color combinations
- Placing on busy backgrounds
- Adding shadows or outlines

### Imagery Style

**Technical Diagrams**
- Clean, vector-based
- Consistent stroke weights
- Use brand colors
- Avoid 3D renders or skeuomorphism

**Photography (if used)**
- High contrast, desaturated
- Abstract/macro shots of materials, structures
- Avoid people, offices, generic tech imagery

**Icons**
- Geometric, consistent stroke weight
- Rounded or square corners (pick one, stay consistent)
- Minimal detail, clear at small sizes

---

## Tone of Voice

### Principles

**Confident, Not Arrogant**
- Security claims are backed by architecture
- Let the tech speak for itself
- Avoid superlatives without substance

**Technical, Not Exclusionary**
- Accessible to developers of all levels
- Explain concepts, don't gatekeep
- Use precise language, not jargon

**Direct, Not Aggressive**
- Get to the point
- Respect the reader's time
- Clear calls to action

**Practical, Not Theoretical**
- Focus on what you can build
- Real examples over abstract concepts
- Show, don't just tell

### Writing Examples

**Good:**
> "Graphene compiles your Dockerfile into a sealed unikernel. No shell, no package manager, no attack surface."

**Avoid:**
> "Our revolutionary next-generation platform leverages cutting-edge unikernel technology to provide unprecedented security."

**Good:**
> "Cold start in 200ms. Your function runs before the user notices."

**Avoid:**
> "Experience blazingly fast performance with our optimized runtime engine."

---

## Taglines

### Primary Options

| Tagline | Emphasis |
|---------|----------|
| "Serverless. Secure. Decentralized." | Core pillars |
| "Where AI agents run safe" | Security for AI |
| "Functions without the attack surface" | Security/minimal |
| "Execute instantly. Settle trustlessly." | Speed + decentralization |

### Contextual Taglines

**For AI/Agent audiences:**
- "The safe runtime for AI agents"
- "Let agents write code. Don't let them run shells."

**For serverless developers:**
- "Lambda speed. No vendor lock-in."
- "Your Dockerfile. Our isolation."

**For crypto/DePIN audiences:**
- "Off-chain execution. On-chain settlement."
- "Compute without consensus lag"

---

## Brand Positioning

### What We Are

- A decentralized serverless platform
- Optimized for AI agent execution
- Security-first by architecture
- Developer-friendly with familiar tools (Dockerfile, SDKs)

### What We Are Not

- A general-purpose cloud provider
- A blockchain (we use Solana for settlement)
- A container orchestrator
- Enterprise middleware

### Competitive Positioning

| vs AWS Lambda | vs Akash | vs Traditional Containers |
|---------------|----------|---------------------------|
| Decentralized, no vendor lock-in | Faster cold starts (200ms vs 30-120s) | Hardware isolation via MicroVM |
| Pay with crypto | AI-safe (no shell access) | Minimal attack surface |
| Permissionless access | Off-chain execution speed | Content-addressed caching |

---

## What to Avoid

### Visual

- Overcomplicated crypto aesthetics (neon gradients, token-heavy imagery)
- Generic cloud imagery (literal clouds, server racks)
- Meme coin vibes (rocket ships, "to the moon")
- Overly corporate stock photography

### Messaging

- "Revolutionary" / "Next-generation" / "Cutting-edge" without specifics
- Blockchain maximalism or tribalism
- FUD about competitors
- Promises we can't back with architecture

### Tone

- Enterprise stuffiness that alienates indie developers
- Hype language from crypto marketing
- Condescension toward less technical users
- False urgency or artificial scarcity

---

## Application Guidelines

### Website

- Clean, minimal design reflecting unikernel philosophy
- Code examples prominent (developers want to see how it works)
- Security messaging above the fold
- Clear path to documentation and SDKs

### Documentation

- Technical accuracy over marketing polish
- Runnable examples
- Honest about limitations (e.g., no TEE yet)
- Migration guides for Lambda users

### Social Media

- Share technical content, not hype
- Engage with developer community authentically
- Highlight real use cases and builders
- Avoid engagement bait

### Developer Relations

- Attend relevant conferences (AI, serverless, Solana)
- Create tutorials and example projects
- Be responsive in Discord/GitHub
- Celebrate community contributions

---

## Brand Assets

*To be added:*
- [ ] Logo files (SVG, PNG at various sizes)
- [ ] Color palette files (for design tools)
- [ ] Typography specifications
- [ ] Icon set
- [ ] Presentation templates
- [ ] Social media templates
