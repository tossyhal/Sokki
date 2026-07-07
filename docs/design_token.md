# Sokki — Design Tokens (Light)

シック&シンプル / 明るめ・白基調。アクセントは深い赤 1色。

## 1. Tailwind — `tailwind.config.js` の `theme.extend`

```js
// tailwind.config.js
module.exports = {
  theme: {
    extend: {
      colors: {
        bg:        '#F6F5F2', // アプリ背景（温かみのあるオフホワイト）
        surface:   '#FFFFFF', // カード・パネル
        'surface-2':'#FBFBFA', // サイドバー・ホバー面
        elevate:   '#EFEEEA', // アクティブ nav / 微差の面
        line:      'rgba(20,20,25,0.08)', // 極細罫線
        'line-strong':'rgba(20,20,25,0.14)', // ボタン枠など
        ink:       '#1B1B1E', // 本文テキスト
        'ink-2':   '#6E6E73', // セカンダリ
        'ink-3':   '#9A9A9F', // メタ / プレースホルダ
        accent:    '#C4453F', // REC / 主アクション
        'accent-hover':'#AE3A35',
        'accent-soft':'rgba(196,69,63,0.10)', // バッジ地
        warn:      '#B0742F', // 要再処理などの注意
        'warn-soft':'rgba(176,116,47,0.12)',
      },
      fontFamily: {
        sans: ['Inter', 'Noto Sans JP', 'system-ui', 'sans-serif'],
      },
      fontSize: {
        // 時間表示（主役）
        'time-lg':  ['56px', { lineHeight: '1', letterSpacing: '-0.02em', fontWeight: '700' }],
        'time-md':  ['30px', { lineHeight: '1', letterSpacing: '-0.02em', fontWeight: '700' }],
        h1:         ['23px', { lineHeight: '1.25', letterSpacing: '-0.01em', fontWeight: '600' }],
        title:      ['15px', { lineHeight: '1.4', fontWeight: '600' }],
        body:       ['14px', { lineHeight: '1.6' }],
        meta:       ['12.5px', { lineHeight: '1.5' }],
        micro:      ['11px', { lineHeight: '1.4', letterSpacing: '0.02em' }],
      },
      borderRadius: {
        chip:   '6px',
        btn:    '7px',
        card:   '8px',
        panel:  '12px',
      },
      spacing: {
        // 4px ベース（Tailwind 既定 + 補助）
        'sidebar': '220px',
      },
      boxShadow: {
        card:  '0 1px 2px rgba(20,20,25,0.05)',
        pop:   '0 24px 60px -28px rgba(20,20,25,0.28)',
        accent:'0 1px 2px rgba(196,69,63,0.35)',
      },
      keyframes: {
        'rec-pulse': { '0%,100%': { opacity: '1' }, '50%': { opacity: '.35' } },
        'seg-in':    { from: { opacity: '0', transform: 'translateY(4px)' }, to: { opacity: '1', transform: 'none' } },
      },
      animation: {
        'rec-pulse': 'rec-pulse 1.6s ease-in-out infinite',
        'seg-in':    'seg-in .28s ease-out',
      },
    },
  },
};
```

`tabular-nums` は経過時間・タイムスタンプに必須:
```html
<span class="font-sans tabular-nums">12:47</span>
```

## 2. CSS 変数（`:root`）

```css
:root {
  /* color */
  --bg: #F6F5F2;
  --surface: #FFFFFF;
  --surface-2: #FBFBFA;
  --elevate: #EFEEEA;
  --line: rgba(20,20,25,0.08);
  --line-strong: rgba(20,20,25,0.14);
  --ink: #1B1B1E;
  --ink-2: #6E6E73;
  --ink-3: #9A9A9F;
  --accent: #C4453F;
  --accent-hover: #AE3A35;
  --accent-soft: rgba(196,69,63,0.10);
  --warn: #B0742F;
  --warn-soft: rgba(176,116,47,0.12);

  /* radius */
  --r-chip: 6px;
  --r-btn: 7px;
  --r-card: 8px;
  --r-panel: 12px;

  /* elevation */
  --shadow-card: 0 1px 2px rgba(20,20,25,0.05);
  --shadow-pop: 0 24px 60px -28px rgba(20,20,25,0.28);

  /* layout */
  --sidebar-w: 220px;
  --win-min: 900px; /* × 600 */
  --win-base: 1100px; /* × 720 */

  /* type */
  --font-sans: 'Inter', 'Noto Sans JP', system-ui, sans-serif;
}
```

## 3. 使用ルール（要点）

- **時間表示が主役** — 経過時間は `time-lg`(56px) を録音中画面で大胆に。リスト内は `tabular-nums` で桁ズレ防止。
- **罫線は極細・低コントラスト** — 常に `--line`(8%)。ボタン枠のみ `--line-strong`(14%)。
- **角丸は控えめ** — chip 6 / btn 7 / card 8 / panel 12。
- **アクセントは1色のみ** — REC・主アクション・「文字起こし中」バッジ・再生位置ハイライトに限定。多用しない。
- **モーション最小** — REC ドットの `rec-pulse`、新規セグメント追記の `seg-in` のみ。
- **状態色** — 完了は無彩色バッジ（`--elevate`）、注意/エラーは `--warn` / `--accent` のソフト地。
