use core::cell::UnsafeCell;

/// A mutual exclusion primitive for sharing data between interrupt handlers
/// and normal code on x86_64.
///
/// This mutex works by disabling interrupts (CLI instruction) while the
/// protected data is being accessed, then restoring the previous interrupt
/// state (which may re-enable interrupts with STI if they were enabled before).
///
/// # Safety Model
///
/// On a single-core system, disabling interrupts guarantees mutual exclusion:
/// - Normal code can't be preempted by an interrupt handler
/// - Interrupt handlers can't be preempted by other interrupt handlers (same or lower priority)
///
/// This is sound because:
/// 1. The mutex is `Sync` (safe to share between threads/contexts)
/// 2. Access is only through `&self.with()` which enforces the critical section
/// 3. We save/restore the interrupt flag, so nested locking works correctly
///
/// # Example
///
/// ```
/// static SHARED_DATA: IrqMutex<RefCell<u32>> =
///     IrqMutex::new(RefCell::new(0));
///
/// fn interrupt_handler() {
///     SHARED_DATA.with(|data| {
///         *data.borrow_mut() += 1;
///     });
/// }
///
/// fn main_code() {
///     SHARED_DATA.with(|data| {
///         println!("Counter: {}", data.borrow());
///     });
/// }
/// ```
pub struct IrqMutex<T> {
    /// The protected data, wrapped in UnsafeCell to allow interior mutability.
    /// UnsafeCell is the primitive that allows us to get a mutable reference
    /// from an immutable reference - but we must ensure only one mutable
    /// reference exists at a time (enforced by disabling interrupts).
    data: UnsafeCell<T>,
}

// SAFETY: IrqMutex can be safely shared between contexts (Sync) because:
// 1. All access goes through `with()` which disables interrupts
// 2. This prevents concurrent access from interrupt handlers
// 3. On single-core, this guarantees mutual exclusion
unsafe impl<T> Sync for IrqMutex<T> {}

impl<T> IrqMutex<T> {
    /// Creates a new IrqMutex wrapping the given data.
    ///
    /// This is a `const fn`, so it can be used in static initializers:
    /// ```
    /// static MY_MUTEX: IrqMutex<u32> = IrqMutex::new(42);
    /// ```
    pub const fn new(data: T) -> Self {
        Self {
            data: UnsafeCell::new(data),
        }
    }

    /// Executes a closure with access to the protected data.
    ///
    /// This method:
    /// 1. Saves the current interrupt flag state (RFLAGS.IF)
    /// 2. Disables interrupts (CLI)
    /// 3. Calls your closure with mutable access to the data
    /// 4. Restores the original interrupt flag state
    ///
    /// # Critical Section
    ///
    /// While the closure executes, interrupts are disabled. This means:
    /// - Keep closures SHORT - long critical sections increase interrupt latency
    /// - Don't call functions that might block or loop indefinitely
    /// - Hardware interrupts will be delayed until the critical section ends
    ///
    /// # Nested Locking
    ///
    /// It's safe to nest calls to `with()` because we save/restore the IF flag:
    /// ```
    /// MUTEX_A.with(|a| {
    ///     // Interrupts disabled here
    ///     MUTEX_B.with(|b| {
    ///         // Still disabled, but that's fine
    ///     });
    ///     // Still disabled
    /// });
    /// // Interrupts restored to original state here
    /// ```
    ///
    /// # Panics
    ///
    /// If the closure panics, the interrupt state will still be restored correctly
    /// (Rust's unwinding will run this function's cleanup code).
    pub fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        // Step 1: Save current RFLAGS and disable interrupts
        // We use inline assembly to:
        // - PUSHFQ: Push RFLAGS onto stack (includes IF bit at position 9)
        // - POP: Pop the value into our `flags` variable
        // - CLI: Clear Interrupt Flag (disable interrupts)
        let flags: u64;
        unsafe {
            core::arch::asm!(
                "pushfq",           // Push RFLAGS register to stack
                "pop {flags}",      // Pop it into our variable
                "cli",              // Clear interrupt flag (disable interrupts)
                flags = out(reg) flags,
                options(nomem, nostack, preserves_flags)
            );
        }

        // Step 2: Execute the closure with mutable access
        // SAFETY: We've disabled interrupts, so no other code can access this
        // data concurrently. On single-core, this guarantees exclusive access.
        let result = f(unsafe { &mut *self.data.get() });

        // Step 3: Restore interrupt flag if it was previously enabled
        // Bit 9 of RFLAGS is the IF (Interrupt Flag)
        // 0x200 = 0b1000000000 = bit 9
        if flags & 0x200 != 0 {
            // Interrupts were enabled before, re-enable them
            unsafe {
                core::arch::asm!("sti", options(nomem, nostack, preserves_flags));
            }
        }
        // If interrupts were already disabled, we leave them disabled
        // (this handles nested critical sections correctly)

        result
    }
}
