<script lang="ts">
    import favicon from '$lib/assets/favicon.svg';

    let { children } = $props();
</script>

<svelte:head>
    <link rel="icon" href={favicon} />
    <title>tgautodocs — Telegram Bot API MCP server</title>
    <meta
        name="description"
        content="tgautodocs is an MCP server that gives AI agents the exact Telegram Bot API, so they build bots without hallucinating methods or fields."
    />
    <meta property="og:type" content="website" />
    <meta property="og:title" content="tgautodocs — Telegram Bot API MCP server" />
    <meta
        property="og:description"
        content="tgautodocs is an MCP server that gives AI agents the exact Telegram Bot API, so they build bots without hallucinating methods or fields."
    />
    <meta name="twitter:card" content="summary" />
    <meta name="twitter:title" content="tgautodocs — Telegram Bot API MCP server" />
    <meta
        name="twitter:description"
        content="tgautodocs is an MCP server that gives AI agents the exact Telegram Bot API, so they build bots without hallucinating methods or fields."
    />
</svelte:head>

{@render children()}
