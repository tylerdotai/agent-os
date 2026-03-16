// SPDX-License-Identifier: MIT
/*
 * Agent OS - Context Manager Kernel Module
 * 
 * Provides syscall interfaces for:
 * - context_allocate: Set token budget for agent
 * - context_add: Add content to context
 * - context_query: Search archived context
 * - context_get_stats: Get context usage stats
 * - agent_spawn: Spawn child agent
 * - agent_send: Message another agent
 * - agent_checkpoint: Save agent state
 */

#include <linux/module.h>
#include <linux/kernel.h>
#include <linux/init.h>
#include <linux/syscalls.h>
#include <linux/sched.h>
#include <linux/uaccess.h>
#include <linux/slab.h>
#include <linux/hash.h>
#include <linux/radix-tree.h>
#include <linux/fs.h>
#include <linux/fcntl.h>
#include <linux/version.h>

MODULE_LICENSE("MIT");
MODULE_AUTHOR("Tyler Delano");
MODULE_DESCRIPTION("Agent OS - Context Manager Kernel Module");
MODULE_VERSION("0.1.0");

// ============================================================================
// Configuration
// ============================================================================

#define AGENT_OS_HASH_BITS 8
#define MAX_CONTEXT_PAGES 1024
#define MAX_CONTENT_SIZE (64 * 1024)  // 64KB per page

// ============================================================================
// Data Structures
// ============================================================================

// Context page structure - represents a chunk of context
struct agent_context_page {
    u64 id;
    char *content;
    size_t len;
    float importance;
    unsigned long last_accessed;
    bool in_memory;
    struct rb_node node;
};

// Per-agent context structure
struct agent_context {
    pid_t pid;
    size_t token_budget;
    size_t token_used;
    struct rb_root page_tree;
    spinlock_t lock;
    struct list_head list;
    char name[64];
};

// Message structure for agent-to-agent communication
struct agent_message {
    u64 id;
    pid_t from_pid;
    pid_t to_pid;
    char *content;
    size_t len;
    unsigned long timestamp;
    bool read;
    struct list_head list;
};

// ============================================================================
// Global State
// ============================================================================

static RADIX_TREE(context_tree, GFP_ATOMIC);
static LIST_HEAD(agent_list);
static LIST_HEAD(message_list);
static DEFINE_SPINLOCK(global_lock);
static u64 next_context_id = 1;
static u64 next_message_id = 1;

// ============================================================================
// Helper Functions
// ============================================================================

static struct agent_context *find_agent_context(pid_t pid)
{
    struct agent_context *ctx;
    list_for_each_entry(ctx, &agent_list, list) {
        if (ctx->pid == pid)
            return ctx;
    }
    return NULL;
}

static struct agent_message *find_message(u64 id)
{
    struct agent_message *msg;
    list_for_each_entry(msg, &message_list, list) {
        if (msg->id == id)
            return msg;
    }
    return NULL;
}

static int context_page_insert(struct agent_context *ctx, struct agent_context_page *page)
{
    struct rb_node **new = &ctx->page_tree.rb_node;
    struct rb_node *parent = NULL;
    struct agent_context_page *this;

    while (*new) {
        this = container_of(*new, struct agent_context_page, node);
        parent = *new;
        if (page->id < this->id)
            new = &((*new)->rb_left);
        else
            new = &((*new)->rb_right);
    }

    rb_link_node(&page->node, parent, new);
    rb_insert_color(&page->node, &ctx->page_tree);
    return 0;
}

static struct agent_context_page *context_page_find(struct agent_context *ctx, u64 id)
{
    struct rb_node *node = ctx->page_tree.rb_node;
    while (node) {
        struct agent_context_page *page = container_of(node, struct agent_context_page, node);
        if (id < page->id)
            node = node->rb_left;
        else if (id > page->id)
            node = node->rb_right;
        else
            return page;
    }
    return NULL;
}

// ============================================================================
// Syscall: context_allocate
// ============================================================================
// Set token budget for an agent
// Args: pid, token_budget
// Returns: 0 on success, negative on error

#if LINUX_VERSION_CODE >= KERNEL_VERSION(4,17,0)
#define AGENT_OS_SYS_CONTEXT_ALLOCATE 451
#else
#error "Kernel version too old"
#endif

SYSCALL_DEFINE2(agent_os_context_allocate, pid_t, pid, size_t, token_budget)
{
    struct agent_context *ctx;
    unsigned long flags;

    if (token_budget > (64 * 1024 * 1024)) {  // Max 64M tokens
        return -EINVAL;
    }

    spin_lock_irqsave(&global_lock, flags);
    
    ctx = find_agent_context(pid);
    if (!ctx) {
        // Create new context for this pid
        ctx = kmalloc(sizeof(struct agent_context), GFP_ATOMIC);
        if (!ctx) {
            spin_unlock_irqrestore(&global_lock, flags);
            return -ENOMEM;
        }
        
        ctx->pid = pid;
        ctx->token_budget = token_budget;
        ctx->token_used = 0;
        ctx->page_tree = RB_ROOT;
        spin_lock_init(&ctx->lock);
        snprintf(ctx->name, sizeof(ctx->name), "agent-%d", pid);
        
        list_add_tail(&ctx->list, &agent_list);
    } else {
        ctx->token_budget = token_budget;
    }
    
    spin_unlock_irqrestore(&global_lock, flags);
    
    printk(KERN_INFO "Agent OS: Allocated %zu tokens for pid %d\n", token_budget, pid);
    return 0;
}

// ============================================================================
// Syscall: context_add
// ============================================================================
// Add content to agent's context
// Args: pid, content_ptr, content_len, importance
// Returns: context page id on success, negative on error

SYSCALL_DEFINE4(agent_os_context_add, pid_t, pid, char __user *, content_ptr, 
                 size_t, content_len, float, importance)
{
    struct agent_context *ctx;
    struct agent_context_page *page;
    unsigned long flags;
    u64 page_id;
    char *content;

    if (content_len > MAX_CONTENT_SIZE)
        return -EINVAL;

    spin_lock_irqsave(&global_lock, flags);
    
    ctx = find_agent_context(pid);
    if (!ctx) {
        // Create context with default budget
        ctx = kmalloc(sizeof(struct agent_context), GFP_ATOMIC);
        if (!ctx) {
            spin_unlock_irqrestore(&global_lock, flags);
            return -ENOMEM;
        }
        
        ctx->pid = pid;
        ctx->token_budget = 8192;  // Default 8K tokens
        ctx->token_used = 0;
        ctx->page_tree = RB_ROOT;
        spin_lock_init(&ctx->lock);
        snprintf(ctx->name, sizeof(ctx->name), "agent-%d", pid);
        
        list_add_tail(&ctx->list, &agent_list);
    }
    
    // Allocate content
    content = kmalloc(content_len + 1, GFP_ATOMIC);
    if (!content) {
        spin_unlock_irqrestore(&global_lock, flags);
        return -ENOMEM;
    }
    
    if (copy_from_user(content, content_ptr, content_len)) {
        kfree(content);
        spin_unlock_irqrestore(&global_lock, flags);
        return -EFAULT;
    }
    content[content_len] = '\0';
    
    // Allocate page
    page = kmalloc(sizeof(struct agent_context_page), GFP_ATOMIC);
    if (!page) {
        kfree(content);
        spin_unlock_irqrestore(&global_lock, flags);
        return -ENOMEM;
    }
    
    page_id = next_context_id++;
    page->id = page_id;
    page->content = content;
    page->len = content_len;
    page->importance = importance;
    page->last_accessed = jiffies;
    page->in_memory = true;
    
    spin_lock(&ctx->lock);
    context_page_insert(ctx, page);
    ctx->token_used += content_len / 4;  // Approximate tokens
    spin_unlock(&ctx->lock);
    
    spin_unlock_irqrestore(&global_lock, flags);
    
    printk(KERN_INFO "Agent OS: Added context page %llu to pid %d (%zu bytes)\n", 
           page_id, pid, content_len);
    return (int)page_id;
}

// ============================================================================
// Syscall: context_query
// ============================================================================
// Query archived context by importance threshold
// Args: pid, min_importance
// Returns: number of pages on success, negative on error

SYSCALL_DEFINE2(agent_os_context_query, pid_t, pid, float, min_importance)
{
    struct agent_context *ctx;
    unsigned long flags;
    int count = 0;
    struct rb_node *node;

    spin_lock_irqsave(&global_lock, flags);
    
    ctx = find_agent_context(pid);
    if (!ctx) {
        spin_unlock_irqrestore(&global_lock, flags);
        return -ENOENT;
    }
    
    spin_lock(&ctx->lock);
    for (node = rb_first(&ctx->page_tree); node; node = rb_next(node)) {
        struct agent_context_page *page = container_of(node, struct agent_context_page, node);
        if (page->importance >= min_importance && page->in_memory)
            count++;
    }
    spin_unlock(&ctx->lock);
    
    spin_unlock_irqrestore(&global_lock, flags);
    
    return count;
}

// ============================================================================
// Syscall: context_get_stats
// ============================================================================
// Get context usage statistics for an agent
// Args: pid
// Returns: token_used on success, negative on error

SYSCALL_DEFINE1(agent_os_context_get_stats, pid_t, pid)
{
    struct agent_context *ctx;
    unsigned long flags;
    size_t token_used;
    size_t token_budget;

    spin_lock_irqsave(&global_lock, flags);
    
    ctx = find_agent_context(pid);
    if (!ctx) {
        spin_unlock_irqrestore(&global_lock, flags);
        return -ENOENT;
    }
    
    spin_lock(&ctx->lock);
    token_used = ctx->token_used;
    token_budget = ctx->token_budget;
    spin_unlock(&ctx->lock);
    
    spin_unlock_irqrestore(&global_lock, flags);
    
    // Return high bits budget, low bits used
    printk(KERN_INFO "Agent OS: pid %d stats - used: %zu/%zu\n", pid, token_used, token_budget);
    return (int)(token_used | (token_budget << 32));
}

// ============================================================================
// Syscall: agent_spawn
// ============================================================================
// Create a new agent process
// Args: name_ptr, name_len
// Returns: new pid on success, negative on error

SYSCALL_DEFINE2(agent_os_agent_spawn, char __user *, name_ptr, int, name_len)
{
    struct task_struct *task;
    struct agent_context *ctx;
    unsigned long flags;
    pid_t new_pid;
    char name[64];

    if (name_len >= sizeof(name))
        return -EINVAL;

    if (copy_from_user(name, name_ptr, name_len))
        return -EFAULT;
    name[name_len] = '\0';

    // Create a new kernel thread for the agent
    task = kthread_create(NULL, NULL, "agent-os-%s", name);
    if (IS_ERR(task))
        return PTR_ERR(task);

    new_pid = task->pid;
    wake_up_process(task);

    // Create context for new agent
    spin_lock_irqsave(&global_lock, flags);
    
    ctx = kmalloc(sizeof(struct agent_context), GFP_ATOMIC);
    if (ctx) {
        ctx->pid = new_pid;
        ctx->token_budget = 8192;
        ctx->token_used = 0;
        ctx->page_tree = RB_ROOT;
        spin_lock_init(&ctx->lock);
        strncpy(ctx->name, name, sizeof(ctx->name) - 1);
        ctx->name[sizeof(ctx->name) - 1] = '\0';
        
        list_add_tail(&ctx->list, &agent_list);
    }
    
    spin_unlock_irqrestore(&global_lock, flags);
    
    printk(KERN_INFO "Agent OS: Spawned agent '%s' with pid %d\n", name, new_pid);
    return new_pid;
}

// ============================================================================
// Syscall: agent_send
// ============================================================================
// Send message to another agent
// Args: from_pid, to_pid, content_ptr, content_len
// Returns: message id on success, negative on error

SYSCALL_DEFINE4(agent_os_agent_send, pid_t, from_pid, pid_t, to_pid, 
                char __user *, content_ptr, size_t, content_len)
{
    struct agent_message *msg;
    unsigned long flags;
    char *content;
    u64 msg_id;

    if (content_len > MAX_CONTENT_SIZE)
        return -EINVAL;

    content = kmalloc(content_len + 1, GFP_ATOMIC);
    if (!content)
        return -ENOMEM;

    if (copy_from_user(content, content_ptr, content_len)) {
        kfree(content);
        return -EFAULT;
    }
    content[content_len] = '\0';

    msg = kmalloc(sizeof(struct agent_message), GFP_ATOMIC);
    if (!msg) {
        kfree(content);
        return -ENOMEM;
    }

    msg_id = next_message_id++;
    msg->id = msg_id;
    msg->from_pid = from_pid;
    msg->to_pid = to_pid;
    msg->content = content;
    msg->len = content_len;
    msg->timestamp = jiffies;
    msg->read = false;

    spin_lock_irqsave(&global_lock, flags);
    list_add_tail(&msg->list, &message_list);
    spin_unlock_irqrestore(&global_lock, flags);

    printk(KERN_INFO "Agent OS: Message %llu sent from %d to %d\n", 
           msg_id, from_pid, to_pid);
    return (int)msg_id;
}

// ============================================================================
// Syscall: agent_receive
// ============================================================================
// Receive pending messages for an agent
// Args: pid
// Returns: message count on success, negative on error

SYSCALL_DEFINE1(agent_os_agent_receive, pid_t, pid)
{
    struct agent_message *msg, *tmp;
    unsigned long flags;
    int count = 0;

    spin_lock_irqsave(&global_lock, flags);
    
    list_for_each_entry_safe(msg, tmp, &message_list, list) {
        if (msg->to_pid == pid && !msg->read) {
            msg->read = true;
            count++;
        }
    }
    
    spin_unlock_irqrestore(&global_lock, flags);
    
    return count;
}

// ============================================================================
// Module Init/Exit
// ============================================================================

static int __init agent_os_init(void)
{
    printk(KERN_INFO "Agent OS: Context manager module loaded\n");
    printk(KERN_INFO "Agent OS: Syscalls registered\n");
    printk(KERN_INFO "Agent OS: Supports context_allocate, context_add, context_query,\n");
    printk(KERN_INFO "Agent OS: context_get_stats, agent_spawn, agent_send, agent_receive\n");
    return 0;
}

static void __exit agent_os_exit(void)
{
    struct agent_context *ctx, *tmp_ctx;
    struct agent_message *msg, *tmp_msg;
    unsigned long flags;

    spin_lock_irqsave(&global_lock, flags);

    // Free all contexts
    list_for_each_entry_safe(ctx, tmp_ctx, &agent_list, list) {
        struct rb_node *node = rb_first(&ctx->page_tree);
        while (node) {
            struct agent_context_page *page = container_of(node, struct agent_context_page, node);
            struct rb_node *next = rb_next(node);
            rb_erase(node, &ctx->page_tree);
            kfree(page->content);
            kfree(page);
            node = next;
        }
        list_del(&ctx->list);
        kfree(ctx);
    }

    // Free all messages
    list_for_each_entry_safe(msg, tmp_msg, &message_list, list) {
        list_del(&msg->list);
        kfree(msg->content);
        kfree(msg);
    }

    spin_unlock_irqrestore(&global_lock, flags);

    printk(KERN_INFO "Agent OS: Context manager module unloaded\n");
}

module_init(agent_os_init);
module_exit(agent_os_exit);
