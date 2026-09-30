// Linen app icon, drawn with Canvas 2D.
//
// All geometry is in a 1024 × 1024 space (Apple's macOS icon grid: an 824 pt
// tile inset 100 pt, with room for its shadow) and scaled to the target size.
// Colours come from src/lib/theme/tokens.ts: Paper and Night grounds, the rust
// accent, and the Night accent for the Night ribbon.

;(function (global) {
  const VARIANTS = {
    paper: {
      tileTop: '#FBF8F2', tileBottom: '#F1E6D2', // Paper → Sepia ground
      thread: 'rgba(140,110,80,0.045)',
      rim: 'rgba(255,255,255,0.9)',
      cover: '#8C4A2F', coverShade: '#6F3923',
      page: '#FFFDF8', pageEdge: '#EDE4D4', pageStack: '#E4D9C6',
      gutter: 'rgba(34,32,28,0.11)',
      ribbon: '#A2502E', ribbonShade: '#7A3A20',
      shadow: 'rgba(60,40,20,0.22)',
    },
    night: {
      tileTop: '#2F2B27', tileBottom: '#1B1A18', // Night panel → Night ground
      thread: 'rgba(255,240,220,0.035)',
      rim: 'rgba(255,255,255,0.10)',
      cover: '#6E3A24', coverShade: '#4E2817',
      page: '#E9E3D8', pageEdge: '#CFC6B7', pageStack: '#BDB3A2',
      gutter: 'rgba(0,0,0,0.2)',
      ribbon: '#D39A73', ribbonShade: '#A8704C',
      shadow: 'rgba(0,0,0,0.5)',
    },
  }

  const TILE = { x: 100, y: 100, s: 824 }
  // Shadow blur and offset ignore the canvas transform, so they are scaled by hand.
  let K = 1
  const CX = 512

  // A continuous-corner rounded square (superellipse, n = 5), close to the
  // macOS Big Sur tile, rather than a plain arc-cornered rect.
  function squircle(ctx, x, y, s) {
    const r = s / 2, cx = x + r, cy = y + r, n = 5, steps = 256
    ctx.beginPath()
    for (let i = 0; i <= steps; i++) {
      const t = (i / steps) * Math.PI * 2
      const c = Math.cos(t), sn = Math.sin(t)
      const px = cx + r * Math.sign(c) * Math.pow(Math.abs(c), 2 / n)
      const py = cy + r * Math.sign(sn) * Math.pow(Math.abs(sn), 2 / n)
      if (i) ctx.lineTo(px, py)
      else ctx.moveTo(px, py)
    }
    ctx.closePath()
  }

  // One half of the open book. `side` is -1 (left) or 1 (right); `grow` pushes
  // the outer and lower edges out, for page layers underneath and the cover.
  function pagePath(ctx, side, grow = 0) {
    const s = side
    const w = 286 + grow
    const top = 326 + grow * 0.5
    const bottom = 668 + grow
    const gutterTop = 352 + grow * 0.5
    const gutterBottom = 704 + grow * 1.1
    ctx.beginPath()
    ctx.moveTo(CX, gutterTop)
    ctx.bezierCurveTo(CX + s * 56, 314 + grow * 0.5, CX + s * 176, 298 + grow * 0.5, CX + s * w, top)
    ctx.lineTo(CX + s * w, bottom)
    ctx.bezierCurveTo(CX + s * 176, 648 + grow, CX + s * 60, 664 + grow, CX, gutterBottom)
    ctx.closePath()
  }

  // A small deterministic PRNG so the weave is identical on every render.
  function rng(seed) {
    return () => ((seed = (seed * 1664525 + 1013904223) >>> 0) / 4294967296)
  }

  function drawTile(ctx, v, detailed) {
    // Drop shadow under the tile.
    ctx.save()
    ctx.shadowColor = v.shadow
    ctx.shadowBlur = 28 * K
    ctx.shadowOffsetY = 12 * K
    squircle(ctx, TILE.x, TILE.y, TILE.s)
    const g = ctx.createLinearGradient(0, TILE.y, 0, TILE.y + TILE.s)
    g.addColorStop(0, v.tileTop)
    g.addColorStop(1, v.tileBottom)
    ctx.fillStyle = g
    ctx.fill()
    ctx.restore()

    if (detailed) {
      // Linen: warp and weft threads, each slightly irregular in weight.
      ctx.save()
      squircle(ctx, TILE.x, TILE.y, TILE.s)
      ctx.clip()
      const rand = rng(7)
      ctx.strokeStyle = v.thread
      for (let p = TILE.y; p < TILE.y + TILE.s; p += 6) {
        ctx.lineWidth = 1 + rand() * 1.4
        ctx.beginPath(); ctx.moveTo(TILE.x, p + rand()); ctx.lineTo(TILE.x + TILE.s, p + rand()); ctx.stroke()
        ctx.lineWidth = 1 + rand() * 1.4
        ctx.beginPath(); ctx.moveTo(p + rand(), TILE.y); ctx.lineTo(p + rand(), TILE.y + TILE.s); ctx.stroke()
      }
      ctx.restore()
    }

    // A light rim along the top edge, for a sheet of card rather than a flat square.
    ctx.save()
    squircle(ctx, TILE.x + 1, TILE.y + 1, TILE.s - 2)
    ctx.clip()
    squircle(ctx, TILE.x, TILE.y + 3, TILE.s)
    ctx.lineWidth = 4
    ctx.strokeStyle = v.rim
    ctx.stroke()
    ctx.restore()
  }

  function drawBook(ctx, v, detailed) {
    // Contact shadow.
    ctx.save()
    ctx.shadowColor = v.shadow
    ctx.shadowBlur = 36 * K
    ctx.shadowOffsetY = 14 * K
    for (const s of [-1, 1]) { pagePath(ctx, s, 26); ctx.fillStyle = v.coverShade; ctx.fill() }
    ctx.restore()

    // Cover board, with a darker spine where the two boards meet.
    for (const s of [-1, 1]) {
      pagePath(ctx, s, 26)
      const g = ctx.createLinearGradient(CX, 0, CX + s * 312, 0)
      g.addColorStop(0, v.coverShade)
      g.addColorStop(0.12, v.cover)
      g.addColorStop(1, v.cover)
      ctx.fillStyle = g
      ctx.fill()
    }

    drawRibbon(ctx, v)

    // Page block: two layers under the open spread.
    for (const [grow, fill] of [[14, v.pageStack], [7, v.pageEdge]]) {
      for (const s of [-1, 1]) { pagePath(ctx, s, grow); ctx.fillStyle = fill; ctx.fill() }
    }
    if (detailed) {
      // Hairlines between the stacked leaves on the outer edges.
      ctx.save()
      ctx.strokeStyle = v.pageStack
      ctx.lineWidth = 2
      for (const s of [-1, 1]) { pagePath(ctx, s, 3.5); ctx.stroke() }
      ctx.restore()
    }

    // The open spread, shaded into the gutter.
    for (const s of [-1, 1]) {
      pagePath(ctx, s, 0)
      ctx.fillStyle = v.page
      ctx.fill()
      ctx.save()
      ctx.clip()
      const g = ctx.createLinearGradient(CX, 0, CX + s * 64, 0)
      g.addColorStop(0, v.gutter)
      g.addColorStop(1, 'rgba(0,0,0,0)')
      ctx.fillStyle = g
      ctx.fillRect(CX - 120, 280, 240, 460)
      ctx.restore()
    }
  }

  // The ribbon comes out from between the leaves and hangs below the book,
  // ending in a swallowtail: the exact place Linen reopens the book at. It is
  // drawn before the pages, which hide its upper end.
  function drawRibbon(ctx, v) {
    const x = CX + 10, w = 44, top = 600, bottom = 846, notch = 28
    ctx.save()
    ctx.shadowColor = v.shadow
    ctx.shadowBlur = 10 * K
    ctx.shadowOffsetY = 4 * K
    ctx.beginPath()
    ctx.moveTo(x, top)
    ctx.lineTo(x + w, top)
    ctx.lineTo(x + w, bottom)
    ctx.lineTo(x + w / 2, bottom - notch)
    ctx.lineTo(x, bottom)
    ctx.closePath()
    const g = ctx.createLinearGradient(x, 0, x + w, 0)
    g.addColorStop(0, v.ribbonShade)
    g.addColorStop(0.35, v.ribbon)
    g.addColorStop(1, v.ribbon)
    ctx.fillStyle = g
    ctx.fill()
    ctx.restore()
  }

  /**
   * Draw the icon into `canvas`.
   * @param {HTMLCanvasElement} canvas
   * @param {{size?: number, variant?: 'paper'|'night', nominal?: number}} opts
   *   `nominal` is the point size the icon is shown at; below 128 the weave and
   *   page hairlines are dropped, as they would only turn into noise.
   */
  function draw(canvas, { size = 1024, variant = 'paper', nominal = size } = {}) {
    const v = VARIANTS[variant] || VARIANTS.paper
    canvas.width = canvas.height = size
    const ctx = canvas.getContext('2d')
    ctx.clearRect(0, 0, size, size)
    ctx.save()
    K = size / 1024
    ctx.scale(K, K)
    const detailed = nominal >= 128
    drawTile(ctx, v, detailed)
    drawBook(ctx, v, detailed)
    ctx.restore()
  }

  global.LinenIcon = { draw, VARIANTS }
})(window)
