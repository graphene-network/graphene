# Graphene Network - Coding Agent Context

## Critical: Unikernels Have NO Shell

**This is the most important concept to understand.**

Unikernels are fundamentally different from containers and VMs. They do NOT have:
- `/bin/bash` or any shell
- `/bin/sh` or any shell alternative
- A way to "SSH into" them
- Users, passwords, or login mechanisms
- The ability to spawn processes
- Package managers (apt, pip, npm at runtime)
- Any way to run arbitrary commands

**If you're writing code that assumes shell access, you're doing it wrong.**

### Why Shells Don't Exist (Architecturally)

1. **Single-Purpose Binary**: A unikernel is compiled as ONE binary containing:
   - Your application code
   - Required libraries (statically linked)
   - Minimal kernel functionality
   - Nothing else

2. **Single-Address-Space**: There's no kernel/userspace separation. The application IS the kernel.

3. **Compile-Time Decisions**: Everything is decided at build time:
   - What syscalls are available
   - What libraries are included
   - What the application does
   - There are no runtime "extras"

4. **Nothing to Shell Into**: The concept doesn't make sense. There's no multi-user system, no process management, no command interpreter.

### Correct Mental Model

```
WRONG (Container thinking):
┌─────────────────────────┐
│ Your App                │
├─────────────────────────┤
│ /bin/bash, /usr/bin/*   │  ← THESE DON'T EXIST
│ pip, apt, curl          │  ← THESE DON'T EXIST
├─────────────────────────┤
│ Linux Kernel (shared)   │
└─────────────────────────┘

CORRECT (Unikernel reality):
┌─────────────────────────┐
│ Your App                │
│ + Required Libraries    │
│ + Minimal Kernel Code   │
│ (one sealed binary)     │
└─────────────────────────┘
         │
         ▼
┌─────────────────────────┐
│ Hypervisor (KVM)        │
└─────────────────────────┘
```

---

## Unikraft Specifics

[Unikraft](https://unikraft.org) is the Library OS framework we use to build unikernels.

### Key Concepts

1. **Micro-libraries**: Composable OS components (schedulers, memory allocators, network stacks)
2. **KConfig Build System**: Select only what you need at compile time
3. **Syscalls = Function Calls**: No context switch, direct invocation
4. **Dead Code Elimination**: Aggressive removal of unused code paths

### What Gets Compiled In

- Only syscalls your app actually uses
- Only libraries your app actually imports
- Only kernel features your app actually needs

### What Does NOT Get Compiled In

- Shell interpreters (no bash, sh, zsh)
- System utilities (no ls, cat, grep, curl)
- Package managers (no apt, pip, npm)
- User management (no passwd, useradd)
- Process spawning (no fork, exec to other programs)
- Debugging tools (no gdb, strace)

---

## Graphene Execution Model

### The Planner/Executor Separation

AI agents in Graphene do NOT get shell access. Instead:

| Layer | Role | Has Shell? |
|-------|------|-----------|
| **Planner (AI)** | Generates Dockerfile + manifest | No |
| **Builder VM** | Compiles to unikernel | Isolated (ephemeral) |
| **Executor** | Runs sealed .unik binary | **No** |

### How Code Runs Without Shell

Binary invocation uses `execve` directly, not shell interpretation:

```python
# WRONG - Shell-based (doesn't work in unikernels):
os.system("ffmpeg -i input.mp4 output.avi")  # Requires /bin/sh
subprocess.run("ffmpeg ...", shell=True)      # Requires /bin/sh

# CORRECT - Direct kernel invocation:
subprocess.run(["/bin/ffmpeg", "-i", "input", "output"], shell=False)
# This calls execve() directly, no shell needed
```

Static binaries (like ffmpeg-static) are included at build time and invoked directly.

### Job Lifecycle

1. User submits: `Dockerfile` + `Kraftfile` + code + payment ticket
2. Ephemeral Builder VM compiles sealed `.unik` binary
3. Builder VM is destroyed (no host access ever)
4. `.unik` runs in Firecracker MicroVM
5. Single entrypoint executes, produces result
6. MicroVM terminates

There is NO interactive session. It's functional/serverless.

---

## Common Mistakes to Avoid

### Mistake 1: Assuming Shell Commands Work
```python
# WRONG
os.system("pip install pandas")  # pip doesn't exist
os.popen("curl https://...").read()  # curl doesn't exist
```

**Fix**: All dependencies must be in the Dockerfile (build-time only).

### Mistake 2: Trying to Spawn Processes
```python
# WRONG
subprocess.Popen(["/bin/bash", "-c", "..."])  # /bin/bash doesn't exist
multiprocessing.Process(target=fn)  # fork() not supported
```

**Fix**: Unikernels are single-process. Use async/threading within the process.

### Mistake 3: Interactive Debugging
```python
# WRONG
import pdb; pdb.set_trace()  # No stdin for interactive debugging
input("Press enter...")  # No interactive input
```

**Fix**: Use logging. Results are captured and returned to the user.

### Mistake 4: Filesystem Assumptions
```python
# WRONG
with open("/etc/passwd", "r") as f:  # Doesn't exist
os.listdir("/bin")  # /bin is empty or doesn't exist
```

**Fix**: Only your application's files exist. Use explicit paths from your code bundle.

### Mistake 5: Network Assumptions
```python
# WRONG
requests.get("https://malware.com")  # Blocked, not in allowlist
socket.connect(("10.0.0.1", 22))  # RFC1918 addresses blocked
```

**Fix**: Only allowlisted egress endpoints work (defined in manifest).

---

## Security Model Summary

| Attack Vector | Container | Unikernel |
|--------------|-----------|-----------|
| Shell injection | Possible | **Impossible** (no shell) |
| Command execution | Via shell | Direct execve only |
| Package tampering | Runtime install | Build-time only |
| Process spawning | Unlimited | **Impossible** |
| Network exfil | Unrestricted | Allowlist only |
| Privilege escalation | Possible | N/A (single address space) |
| Host escape | Kernel exploit | Hypervisor exploit (much harder) |

---

## Quick Reference

### Unikraft Resources
- Docs: https://unikraft.org/docs
- Concepts: https://unikraft.org/docs/concepts
- Security: https://unikraft.org/docs/concepts/security

### Graphene Docs
- Whitepaper: `docs/WHITEPAPER.md`
- ELI5 Explanation: `docs/ELI5.md`
- Endgame Vision: `docs/ENDGAME.md`

### Key Insight
> "The AI agent does not 'run' inside a runtime. It *requests* a build, and the system executes a sealed, single-purpose unikernel."
> — Graphene Whitepaper

---

## Kernel Library

Pre-built unikernels for common runtimes are managed through the kernel library system.

### Supported Runtimes

| Runtime | Versions | Source |
|---------|----------|--------|
| Python  | 3.10, 3.12 | [Unikraft Catalog](https://github.com/unikraft/catalog) |
| Node.js | 20, 21 | [Unikraft Catalog](https://github.com/unikraft/catalog) |
| Bun     | 1.1 | [Unikraft Catalog](https://github.com/unikraft/catalog) |

**Important**: Only versions available in the Unikraft catalog are supported. Check `kernels/kernel-matrix.toml` for current versions.

### Key Files

| Path | Description |
|------|-------------|
| `kernels/kernel-matrix.toml` | Version matrix defining supported runtimes |
| `kernels/<runtime>/<version>/Kraftfile.yaml` | Unikraft build configuration |
| `crates/node/src/kernel/` | Rust kernel registry implementation |
| `.github/workflows/kernel-build.yml` | CI workflow for building kernels |

### Kraftfile Format

Use the modern `runtime:` directive (pulls from Unikraft catalog):

```yaml
spec: v0.6
runtime: unikraft.org/python:3.12
targets:
  - platform: fc
    architecture: x86_64
cmd: ["/usr/bin/python3", "/app/main.py"]
```

**Do NOT use** the older `libraries:` approach - it requires package versions that may not exist in kraft's index.

### Adding New Runtimes

1. Check [Unikraft Catalog](https://github.com/unikraft/catalog) for availability
2. Add version to `kernels/kernel-matrix.toml`
3. Create `kernels/<runtime>/<version>/Kraftfile.yaml`
4. Push to trigger CI build

---

## Infrastructure Trait Pattern

All infrastructure integrations (Firecracker, Unikraft, Iroh, etc.) **must** be defined as traits to enable mock implementations for testing.

### Required Structure

```
crates/node/src/{component}/
├── mod.rs       # Trait definition + error types
├── {impl}.rs    # Real implementation (e.g., firecracker.rs, linux.rs)
└── mock.rs      # Mock implementation for tests
```

### Example Pattern (from `crates/node/src/vmm/`)

**1. Trait Definition (`mod.rs` or `types.rs`):**
```rust
#[async_trait]
pub trait Virtualizer: Send + Sync {
    async fn configure(&mut self, vcpu: u8, mem_mib: u16) -> Result<(), VmmError>;
    async fn start(&mut self) -> Result<(), VmmError>;
    async fn wait(&mut self) -> Result<(), VmmError>;
    async fn shutdown(&mut self) -> Result<(), VmmError>;
}
```

**2. Mock Implementation (`mock.rs`):**
```rust
#[derive(Clone)]
pub enum MockBehavior {
    HappyPath,
    BootFailure,
    KernelPanic,
    InfiniteLoop,
}

pub struct MockVirtualizer {
    behavior: MockBehavior,
    // ... state tracking
}

#[async_trait]
impl Virtualizer for MockVirtualizer {
    // Implement trait methods with configurable behavior
}
```

### Current Trait Implementations

| Module | Trait | Real Impl | Mock |
|--------|-------|-----------|------|
| `vmm` | `Virtualizer` | `firecracker.rs` | `mock.rs` |
| `builder` | `DriveBuilder` | `linux.rs` | `mock.rs` |
| `cache` | `DependencyCache` | `local.rs`, `iroh.rs` | `mock.rs` |

### Rules

1. **Never call infrastructure directly** — always go through the trait
2. **Traits must be `Send + Sync`** — enables async and concurrent usage
3. **Mock behaviors must cover failure modes** — boot failures, crashes, timeouts
4. **Use dependency injection** — pass `impl Trait` or `Box<dyn Trait>` to components

## Task Tracking

This project uses **bd** (beads) for issue tracking. Run `bd onboard` to get started.

### Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --status in_progress  # Claim work
bd close <id>         # Complete work
bd sync               # Sync with git
```

### Landing the Plane (Session Completion)

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO REMOTE** - This is MANDATORY:
   ```bash
   git pull --rebase
   bd sync
   git push
   git status  # MUST show "up to date with origin"
   ```
5. **Clean up** - Clear stashes, prune remote branches
6. **Verify** - All changes committed AND pushed
7. **Hand off** - Provide context for next session

**CRITICAL RULES:**
- Work is NOT complete until `git push` succeeds
- NEVER stop before pushing - that leaves work stranded locally
- NEVER say "ready to push when you are" - YOU must push
- If push fails, resolve and retry until it succeeds

---

## Pull Request Guidelines

### Always Reference GitHub Issues

**MANDATORY**: Every PR description MUST reference relevant GitHub issues.

**Format:**
```markdown
## Summary
[Brief description of changes]

## Changes
- [List of key changes]

## Test plan
- [x] Tests pass
- [x] No clippy warnings
- [ ] Manual testing completed

Closes #123
Closes #456
Related to #789
```

**Keywords that auto-close issues:**
- `Closes #N`
- `Fixes #N`
- `Resolves #N`

**For partial progress:**
- `Related to #N`
- `Partial progress on #N`
- `Blocked by #N`

**Best Practices:**
1. Review open issues BEFORE starting work to find relevant issue numbers
2. Create new issues if no existing issue covers the work
3. Reference ALL issues that the PR addresses (even partially)
4. Use "Closes" only when the issue is fully resolved
5. Update issue status with `gh issue close N --comment "..."` if auto-close doesn't work

### Creating GitHub Issues

**Always add relevant labels** when creating GitHub issues to improve discoverability and triage.

**IMPORTANT: Fetch existing labels first** to avoid creating duplicates or using non-existent labels:

```bash
# FIRST: Check what labels exist in the repository
gh label list

# THEN: Create issue with labels that actually exist
gh issue create --title "..." --body "..." --label "bug" --label "area/networking"

# Add labels to existing issue
gh issue edit <issue-number> --add-label "priority/high"
```

**Common labels:**
- **Type**: `bug`, `feature`, `enhancement`, `documentation`, `refactor`
- **Area**: `area/networking`, `area/vmm`, `area/builder`, `area/p2p`
- **Priority**: `priority/critical`, `priority/high`, `priority/medium`, `priority/low`
- **Status**: `blocked`, `needs-review`, `good-first-issue`

**Best Practices:**
1. **Always run `gh label list` first** — use only labels that exist in the repo
2. Use at least one type label and one area label
3. Add priority labels for bugs and time-sensitive work
4. Use `good-first-issue` for newcomer-friendly tasks
5. If a needed label doesn't exist, create it with `gh label create "name" --description "..." --color "hex"`
