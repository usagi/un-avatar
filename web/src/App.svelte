<script lang="ts">
  import { onMount } from 'svelte';
  import { products } from './lib/products';

  const pillars = products.filter((product) => product.group === 'pillar');
  const tools = products.filter((product) => product.group === 'tool');
  const fallbackRelease = '2.0.0';
  const vccRepoUrl = 'https://usagi.github.io/un-avatar/vcc/index.json';
  const vccAddRepoUrl = `vcc://vpm/addRepo?url=${encodeURIComponent(vccRepoUrl)}`;
  let latestRelease = fallbackRelease;
  let releaseUrl = 'https://github.com/usagi/un-avatar/releases/latest';

  const capabilities = [
    ['VRM / VRC', 'VRM と VRC / Unity アバターをデスクトップ / OBS 向け Renderer へ。'],
    ['MToon / lilToon', 'VRM の MToon と、VRC モデルでよく使われる lilToon 表現に対応。'],
    ['SpringBone / PhysBone', 'VRM SpringBone と VRC PhysBone 由来の揺れものを Renderer で再生。'],
    ['Modular Avatar', 'Modular Avatar を含む Unity 上の構成を Exporter で .unavatar へ。'],
    ['Wardrobe', '衣装や見た目プリセットを、Renderer 起動中にお着替え機能 Wardrobe で切り替え。'],
    ['Spout2 / OBS', '透過つき Spout2 Sender と Window Preview で配信画面へ。']
  ];

  const compatibility = [
    ['MToon', 'VRM material'],
    ['SpringBone', 'VRM physics'],
    ['lilToon', 'VRC shader'],
    ['PhysBone', 'VRC physics'],
    ['Modular Avatar', 'Unity workflow']
  ];

  const flow = ['U.N. Motion', 'U.N. Avatar', 'Spout2 / Window', 'OBS / Stream'];

  const credits = [
    {
      label: 'Avatar model',
      name: 'オリジナル3Dモデル「瑞希」',
      owner: 'Paryi / IKUSIA',
      href: 'https://booth.pm/ja/items/5132797'
    },
    {
      label: 'Outfit',
      name: 'Noble Trace - Classic',
      owner: 'VELLIE',
      href: 'https://booth.pm/ja/items/6786314'
    },
    {
      label: 'Outfit',
      name: 'Field Drape',
      owner: 'CYCR',
      href: 'https://booth.pm/ko/items/8362173'
    },
    {
      label: 'Game capture',
      name: '魔王城ものがたり',
      owner: '©KAIROSOFT CO.,LTD.',
      href: 'https://store.steampowered.com/app/4212210/_/?l=japanese'
    }
  ];

  onMount(() => {
    fetch('https://api.github.com/repos/usagi/un-avatar/releases/latest')
      .then((response) => (response.ok ? response.json() : Promise.reject()))
      .then((release: { tag_name?: string; html_url?: string }) => {
        latestRelease = release.tag_name?.replace(/^v/, '') || fallbackRelease;
        releaseUrl = release.html_url || releaseUrl;
      })
      .catch(() => {
        latestRelease = fallbackRelease;
      });
  });
</script>

<svelte:head>
  <meta property="og:title" content="U.N. Avatar" />
  <meta
    property="og:description"
    content="お気に入りのアバターを、そのまま配信へ。VRM と VRC / Unity アバターをデスクトップ / OBS 向けに表示する仮想アバターレンダラー。"
  />
  <meta property="og:type" content="website" />
  <meta property="og:url" content="https://usagi.github.io/un-avatar/" />
</svelte:head>

<header class="site-header">
  <div class="shell nav-shell">
    <a class="brand" href="https://usagi.network/" aria-label="USAGI.NETWORK home">
      <img src="/un-avatar/assets/brand/un-logo-2026c1.png" alt="" />
      <span>
        <strong>U.N. Apps</strong>
        <small>USAGI.NETWORK</small>
      </span>
    </a>

    <nav class="product-tabs" aria-label="U.N. Apps">
      {#each pillars as product}
        <a
          class:active={product.active}
          href={product.href}
          style={`--tab-accent: ${product.accent}`}
          aria-current={product.active ? 'page' : undefined}
        >
          <span>{product.shortName}</span>
          <small>{product.role}</small>
        </a>
      {/each}
    </nav>

    <nav class="tool-links" aria-label="U.N. Tools">
      {#each tools as product}
        <a href={product.href}>{product.shortName}</a>
      {/each}
    </nav>
  </div>
</header>

<main>
  <section class="hero">
    <div class="shell hero-grid">
      <div class="hero-copy">
        <p class="eyebrow">U.N. Avatar 2.0.0</p>
        <h1>お気に入りのアバターを、そのまま配信へ。</h1>
        <p class="lead">
          VRM と VRC / Unity アバターを、デスクトップ / OBS 向けに表示する仮想アバターレンダラー。
        </p>
        <p class="hero-body">
          Unity のお気に入りアバターを <code>.unavatar</code> にして、MToon / SpringBone、
          lilToon / PhysBone、Modular Avatar、Wardrobe、Spout2、透過ウィンドウ、
          U.N. Motion / VMC 入力と組み合わせて使えます。
        </p>
        <div class="compat-strip" aria-label="Supported avatar compatibility">
          {#each compatibility as item}
            <span>
              <strong>{item[0]}</strong>
              <small>{item[1]}</small>
            </span>
          {/each}
        </div>
        <div class="actions">
          <a class="button primary" href={releaseUrl}>
            Download {latestRelease}
          </a>
          <a class="button" href="https://github.com/usagi/un-avatar/blob/main/docs/v2-getting-started.md">
            Getting Started
          </a>
        </div>
      </div>

      <div class="hero-visual" aria-label="U.N. Avatar product visual">
        <div class="visual-window">
          <div class="window-bar">
            <span></span>
            <strong>U.N. Avatar</strong>
            <em>Renderer + Supervisor</em>
          </div>
          <div class="visual-content">
            <img
              class="visual-avatar"
              src="/un-avatar/assets/brand/un-avatar-artwork-renderer.png"
              alt="U.N. Avatar Renderer artwork"
            />
            <div class="visual-panel">
              <img src="/un-avatar/assets/brand/un-avatar-artwork-supervisor.png" alt="" />
              <div>
                <strong>Spout2 + Wardrobe</strong>
                <span>ready for stream</span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  </section>

  <section class="media-showcase">
    <div class="shell media-grid">
      <div class="media-copy">
        <p class="eyebrow">Wardrobe on stream</p>
        <h2>衣装切り替えも、配信画面の一部に。</h2>
        <p>
          VRC / Unity アバターの lilToon、PhysBone、Modular Avatar 由来の情報を
          <code>.unavatar</code> として読み込み、Renderer 起動中にお着替え機能 Wardrobe で切り替えられます。
        </p>
      </div>
      <div class="demo-frame">
        <video
          src="/un-avatar/assets/media/wardrobe-demo.webm"
          poster="/un-avatar/assets/media/wardrobe-demo-poster.webp"
          autoplay
          muted
          loop
          playsinline
        ></video>
      </div>
      <figure class="obs-shot">
        <img src="/un-avatar/assets/media/obs-spout2.webp" alt="OBS preview using U.N. Avatar Spout2 output" />
        <figcaption>Spout2 Capture で OBS へ。背景透過やデスクトップ表示にも対応。</figcaption>
      </figure>
      <div class="media-credits" aria-label="Media credits">
        <span>Media credits</span>
        {#each credits as credit}
          <a href={credit.href}>{credit.name} / {credit.owner}</a>
        {/each}
      </div>
    </div>
  </section>

  <section class="capability-band">
    <div class="shell capability-grid">
      {#each capabilities as capability}
        <article>
          <h2>{capability[0]}</h2>
          <p>{capability[1]}</p>
        </article>
      {/each}
    </div>
  </section>

  <section class="shell stream-flow">
    <div>
      <p class="eyebrow">Streaming pipeline</p>
      <h2>U.N. シリーズで、モーションから配信画面までつなぐ。</h2>
    </div>
    <div class="flow-rail" aria-label="U.N. Avatar streaming flow">
      {#each flow as item}
        <span>{item}</span>
      {/each}
    </div>
  </section>

  <section class="shell install-strip">
    <div>
      <p class="eyebrow">Unity Exporter</p>
      <h2>VCC Package Manager</h2>
      <p>VRChat Creator Companion に repository を追加すると、Unity Exporter を導入できます。</p>
      <div class="install-actions">
        <a class="button primary" href={vccAddRepoUrl}>Add to VCC</a>
        <a class="button" href={vccRepoUrl}>Repository JSON</a>
      </div>
    </div>
    <code>{vccRepoUrl}</code>
  </section>

</main>

<footer>
  <div class="shell footer-shell">
    <span>U.N. Avatar by USAGI.NETWORK</span>
    <a href="https://github.com/usagi/un-avatar">GitHub</a>
    <a href="https://github.com/usagi/un-avatar#readme">README</a>
    <a href="https://github.com/usagi/un-avatar/blob/main/docs/third-party-licenses.md">Licenses</a>
  </div>
</footer>
