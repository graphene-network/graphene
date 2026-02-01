This is a "Prompt Playbook" designed to guide a coding agent (like Cursor, Windsurf, or Devin) to build the **Talos** Proof of Concept.

Since coding agents work best when given **discrete, testable chunks**, this roadmap breaks the project into 4 isolated phases. You should complete each phase and verify it works before moving to the next.

### **Prerequisites (Do this manually first)**

Before talking to the agent, ensure your environment is ready:

1. **Install Rust:** `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
2. **Install Solana & Anchor:** Follow the official Anchor installation guide.
3. **Get Firecracker:** Download the `firecracker` binary and place it in your `$PATH`.
4. **Download a Kernel:** Download a "Hello World" kernel (vmlinux) and rootfs (getting an agent to compile a Linux kernel is a recipe for failure). Use the official Firecracker CI artifacts or the `hello-world` examples.

---

### **Phase 1: The "Engine" (Firecracker Wrapper)**

**Goal:** Create a Rust program that can boot a Firecracker MicroVM programmatically.

**Prompt 1: Project Setup & VMM Config**

> "Create a new Rust project named `talos-worker`. I need to use the `firecracker-rs` crate (or similar compatible bindings) to act as a VMM.
> Please write a struct `UnikernelEngine` that accepts paths to a `kernel_image` and a `rootfs_image`. Implement a method `boot()` that configures the Firecracker VMM via its API socket or direct bindings.
> The configuration should be minimal: 1 vCPU, 128MB RAM, and no network interfaces for now. Use standard error handling."

**Prompt 2: The Execution Logic**

> "Now, extend `UnikernelEngine`. Add a function `run_with_script(script_path: PathBuf)`.
> Since we cannot easily modify the rootfs at runtime yet, for this step, just focus on booting the VM. Ensure the VMM waits for the process to exit and returns the status. Write a `main.rs` that hardcodes the path to my local `vmlinux` and `rootfs.img` and boots it to prove it works."

* **Verification:** Run `cargo run`. You should see Firecracker logs and the VM booting up.

---

### **Phase 2: The "Builder" (Disk Image Injection)**

**Goal:** Dynamically create a disk image containing the user's Python script.

**Prompt 3: Filesystem Manipulation**

> "I need a module named `image_builder`. I need a function `create_payload_disk(script_content: &str) -> PathBuf`.
> This function should:
> 1. Create a temporary file (e.g., `payload.ext4`).
> 2. Format it as an ext4 filesystem.
> 3. Mount it (or use a library like `ext4-rs` or standard Linux `dd`/`mkfs` commands via `std::process::Command` if running as root) to write the `script_content` into a file named `agent.py` inside that image.
> 4. Return the path to the new image.
> 
> 
> Note: Assume this runs on Linux. Handle permissions gracefully."

**Prompt 4: Integrating Builder with Engine**

> "Update `UnikernelEngine`. Modify the boot configuration to attach this new `payload.ext4` as a **secondary read-only drive** (`/dev/vdb`).
> You will need to assume the Guest Kernel is configured to mount `/dev/vdb` and run the script inside it. (Note: I will handle the Guest Kernel init script manually).
> Just ensure the Firecracker config adds this second drive."

* **Verification:** Run the code. It should generate a `.ext4` file and boot Firecracker with two drives attached.

---

### **Phase 3: The "Coordinator" (Solana Anchor Program)**

**Goal:** A smart contract to post jobs and accept results.

**Prompt 5: The Anchor Program**

> "Create a new Anchor project named `talos-coordinator`.
> I need a program with two instructions:
> 1. `post_job(ipfs_hash: String, reward: u64)`: Creates a `Job` account storing the requester's key and the hash.
> 2. `submit_result(result_hash: String)`: Updates the `Job` account with the result and marks it as 'Completed'.
> 
> 
> The `Job` account should store: `requester` (Pubkey), `worker` (Pubkey), `input_hash` (String), `output_hash` (String), and `is_complete` (bool)."

**Prompt 6: The Client Client (Rust)**

> "In the `talos-worker` Rust project (from Phase 1), add `anchor-client` and `solana-sdk` dependencies.
> Write a module `chain_listener`. It should:
> 1. Connect to the Solana Devnet.
> 2. Subscribe to `Job` account creation events (or poll for new accounts owned by our program).
> 3. When a new Job is found, print 'Job Found: [Hash]' to the console."
> 
> 

* **Verification:** Deploy the Anchor program to Devnet. Run the worker. Manually trigger a transaction (using a test script) and see if the worker detects it.

---

### **Phase 4: The Integration (End-to-End)**

**Goal:** Connect the Chain Listener to the Unikernel Engine.

**Prompt 7: Tying it Together**

> "Let's connect the pieces in `main.rs`.
> Implement the main event loop:
> 1. `chain_listener` detects a new Job (containing an IPFS hash).
> 2. (Mock Step) Instead of real IPFS, just treat the string as raw Python code for now.
> 3. Call `image_builder::create_payload_disk(code)`.
> 4. Call `UnikernelEngine::run_with_script(...)`.
> 5. Capture the output (mock the output capture for now by just assuming success).
> 6. Call the Anchor `submit_result` instruction to close the loop on-chain."
> 
> 

---

### **Summary of the Files You Will Have**

By the end of this, your directory should look like this:

* `/talos-coordinator` (The Solana Smart Contract)
* `programs/talos/src/lib.rs`


* `/talos-worker` (The Rust Node)
* `src/main.rs` (The loop)
* `src/vmm.rs` (Firecracker logic)
* `src/builder.rs` (Disk image logic)
* `src/chain.rs` (Solana listener)



This gives you a working "Steel Thread"—a single line of code execution that goes from **Blockchain -> Rust Node -> MicroVM -> Blockchain**. Once this PoC is working, you can iterate to add real TEE support and complex Python dependencies.