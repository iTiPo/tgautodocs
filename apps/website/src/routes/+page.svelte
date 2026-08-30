<script lang="ts">
    const MCP_URL = 'https://api.tgautodocs.dev/mcp';

    let copied = $state(false);

    async function copyUrl() {
        try {
            await navigator.clipboard.writeText(MCP_URL);
            copied = true;
            setTimeout(() => (copied = false), 2000);
        } catch {
            // Clipboard can be unavailable; the URL stays visible so it can be copied manually.
        }
    }
</script>

<main>
    <h1>tgautodocs</h1>

    <p class="lead">
        An MCP server that gives AI agents the exact Telegram Bot API, so they build bots correctly
        — without hallucinating methods or fields.
    </p>

    <section>
        <h2>Get started</h2>
        <p>Point your agent at this MCP server:</p>
        <div class="url-row">
            <code>{MCP_URL}</code>
            <button type="button" onclick={copyUrl}>{copied ? 'Copied' : 'Copy'}</button>
        </div>
    </section>
</main>

<style>
    :root {
        color-scheme: light dark;
    }

    :global(body) {
        margin: 0;
        background: #f7f7f8;
        background: light-dark(#f7f7f8, #1a1a1e);
        color: #1a1a1e;
        color: light-dark(#1a1a1e, #eaeaef);
        font-family:
            system-ui,
            -apple-system,
            'Segoe UI',
            Roboto,
            sans-serif;
    }

    main {
        max-width: 40rem;
        margin: 0 auto;
        padding: 4rem 1.5rem;
        display: flex;
        flex-direction: column;
        gap: 2rem;
    }

    h1 {
        font-size: 2.5rem;
        margin: 0;
    }

    .lead {
        font-size: 1.25rem;
        line-height: 1.5;
        margin: 0;
    }

    section {
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
    }

    h2 {
        font-size: 0.875rem;
        text-transform: uppercase;
        letter-spacing: 0.08em;
        margin: 0;
    }

    p {
        margin: 0;
    }

    .url-row {
        display: flex;
        align-items: stretch;
        gap: 0.5rem;
        flex-wrap: wrap;
    }

    code {
        font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
        background: #ffffff;
        background: light-dark(#ffffff, #26262b);
        border: 1px solid #dcdcdf;
        border: 1px solid light-dark(#dcdcdf, #3a3a40);
        border-radius: 0.5rem;
        padding: 0.75rem 1rem;
        overflow-wrap: anywhere;
        flex: 1 1 auto;
    }

    button {
        font: inherit;
        border: 1px solid #1a1a1e;
        border: 1px solid light-dark(#1a1a1e, #eaeaef);
        background: #1a1a1e;
        background: light-dark(#1a1a1e, #eaeaef);
        color: #ffffff;
        color: light-dark(#ffffff, #1a1a1e);
        border-radius: 0.5rem;
        padding: 0.75rem 1rem;
        cursor: pointer;
    }

    button:focus-visible {
        outline: 2px solid #1a1a1e;
        outline: 2px solid light-dark(#1a1a1e, #eaeaef);
        outline-offset: 2px;
    }
</style>
