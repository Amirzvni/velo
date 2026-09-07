# Velo design tokens (cyberpunk 2077)

--velo-bg:        #0D0D0D
--velo-surface:   #141414
--velo-surface-2: #1C1C1C
--velo-border:    #2A2A2A
--velo-yellow:    #FCEE0A   /* primary action, progress fill */
--velo-cyan:      #00F0FF   /* links, secondary accent */
--velo-magenta:   #FF003C   /* errors, stop, danger */
--velo-green:     #39FF14   /* completed */
--velo-text:      #E6E6E6
--velo-text-dim:  #8A8A8A

Rules:
- Yellow on black is the signature pair. Yellow is never used for text bodies, only accents/actions.
- Sharp corners (radius 0-2px), 1px hard borders, no soft shadows.
- Fonts: headings/labels uppercase condensed (Rajdhani), numbers monospace (JetBrains Mono).
- Motion: fast (120ms), linear or steps(). Glitch effect only on hover of primary actions.
- Scanline overlay at 3% opacity, toggleable in settings (off = plain dark theme for accessibility).
