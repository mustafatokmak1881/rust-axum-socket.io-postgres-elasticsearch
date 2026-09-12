from pathlib import Path

ROOT = Path(r"C:/Users/Mustafa1/Documents/projects/rust/rust-axum-full/src/web")

DEFS = """  <defs>
    <pattern id="{p}thatch" width="8" height="8" patternUnits="userSpaceOnUse">
      <rect width="8" height="8" fill="#a98442"/>
      <path d="M1 0v8M5 0v8" stroke="#d3b36a" stroke-width="2"/>
      <path d="M0 6h8" stroke="#77582f" stroke-width="1"/>
    </pattern>
    <pattern id="{p}stone" width="18" height="12" patternUnits="userSpaceOnUse">
      <rect width="18" height="12" fill="#9b9b88"/>
      <path d="M0 0h18M0 12h18M9 0v6M0 6h18M3 6v6" stroke="#666d61" fill="none"/>
    </pattern>
    <pattern id="{p}wood" width="12" height="12" patternUnits="userSpaceOnUse">
      <rect width="12" height="12" fill="#9b7849"/>
      <path d="M0 2h12M0 8h12" stroke="#685134" stroke-width="2"/>
    </pattern>
  </defs>"""


def wrap(name: str, body: str, prefix: str) -> str:
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="160" height="140" viewBox="0 0 160 140">
{DEFS.format(p=prefix)}
  <!-- {name} -->
  <g stroke="#4e4833" stroke-width="1.5" stroke-linejoin="round">
    <ellipse cx="80" cy="122" rx="62" ry="12" fill="#273c25" opacity=".28" stroke="none"/>
{body}
  </g>
</svg>
"""


arts = {
    "headquarters": (
        "Bey otağı",
        """
    <path d="M27 70L79 52L118 73L68 94Z" fill="#d4bf87"/>
    <path d="M27 70V107L68 126V94Z" fill="#b5a073"/>
    <path d="M68 94L118 73V109L68 126Z" fill="#d9c79a"/>
    <path d="M17 73L54 30L84 43L69 96Z" fill="url(#hqthatch)"/>
    <path d="M54 30L124 63L69 96L84 43Z" fill="#b49550"/>
    <path d="M54 30L124 63M69 96L124 63" fill="none" stroke="#624b29" stroke-width="3"/>
    <path d="M35 81V110M55 91V119M76 95V121M100 84V114" fill="none" stroke="#615039" stroke-width="4"/>
    <path d="M29 98L67 115M72 109L116 94" fill="none" stroke="#615039" stroke-width="3"/>
    <path d="M83 121V101Q90 91 97 97V117Z" fill="#4d4533"/>
    <path d="M112 37L134 29L149 39V103L129 115L112 104Z" fill="url(#hqstone)"/>
    <path d="M112 37L129 46V115" fill="none"/>
    <path d="M112 37V24L118 22V29L124 27V20L131 18V26L137 24V19L149 24V39L129 46Z" fill="#b1b0a0"/>
    <path d="M136 55V68M136 81V93" stroke="#353e38" stroke-width="5"/>
    <path d="M135 21V4" stroke="#5d4d31" stroke-width="2"/>
    <path d="M136 4L155 8L136 13Z" fill="#8c3026"/>
""",
        "hq",
    ),
    "timber": (
        "Oduncu",
        """
    <path d="M25 72L77 53L123 76L72 98Z" fill="#ad8c52"/>
    <path d="M25 72V105L72 126V98Z" fill="url(#tmwood)"/>
    <path d="M72 98L123 76V108L72 126Z" fill="#b69b67"/>
    <path d="M16 77L54 33L81 45L72 99Z" fill="url(#tmthatch)"/>
    <path d="M54 33L131 70L72 99L81 45Z" fill="url(#tmthatch)"/>
    <path d="M54 33L131 70M16 77L72 99" fill="none" stroke="#64482a" stroke-width="3"/>
    <path d="M83 120V96L106 87V113Z" fill="#4d4934"/>
    <path d="M33 86V108M58 96V120" stroke="#55462f" stroke-width="4"/>
    <g transform="translate(10 112) rotate(-12)">
      <rect x="0" y="0" width="36" height="7" rx="3" fill="#785431"/>
      <ellipse cx="36" cy="3.5" rx="4" ry="3.5" fill="#d0ad6b"/>
      <rect x="4" y="-9" width="36" height="7" rx="3" fill="#886239"/>
      <ellipse cx="40" cy="-5.5" rx="4" ry="3.5" fill="#d0ad6b"/>
    </g>
    <path d="M136 53V104" stroke="#655536" stroke-width="7"/>
    <path d="M136 10L113 58H159Z" fill="#4f6939"/>
    <path d="M136 28L111 77H161Z" fill="#3e5931"/>
    <path d="M136 43L113 91H159Z" fill="#476338"/>
""",
        "tm",
    ),
    "warehouse": (
        "Ambar",
        """
    <path d="M24 67L83 45L137 69L77 96Z" fill="#d5c395"/>
    <path d="M24 67V105L77 128V96Z" fill="#ad9463"/>
    <path d="M77 96L137 69V106L77 128Z" fill="#d2bc84"/>
    <path d="M14 71L48 27L79 41L77 98Z" fill="url(#whthatch)"/>
    <path d="M48 27L146 65L77 98L79 41Z" fill="url(#whthatch)"/>
    <path d="M48 27L146 65M14 71L77 98" fill="none" stroke="#694e2b" stroke-width="3"/>
    <path d="M33 81V109M56 92V119M84 96V124M124 78V111" stroke="#5d4d32" stroke-width="4"/>
    <path d="M92 121V98L115 88V113Z" fill="url(#whwood)"/>
    <ellipse cx="22" cy="116" rx="9" ry="11" fill="#c5ae75"/>
    <ellipse cx="36" cy="120" rx="8" ry="10" fill="#bfa36c"/>
    <ellipse cx="50" cy="117" rx="7" ry="9" fill="#c8b17a"/>
""",
        "wh",
    ),
    "barracks": (
        "Kışla",
        """
    <path d="M18 78L80 48L142 78L80 108Z" fill="#c4a46a"/>
    <path d="M18 78V108L80 132V108Z" fill="url(#brwood)"/>
    <path d="M80 108L142 78V108L80 132Z" fill="#b8955f"/>
    <path d="M28 78L80 22L132 78Z" fill="url(#brthatch)"/>
    <path d="M28 78L80 22L132 78" fill="none" stroke="#624b29" stroke-width="2.5"/>
    <path d="M70 132V100H90V132Z" fill="#4d4533"/>
    <path d="M40 88V112M55 95V118M105 95V118M120 88V112" stroke="#5a4a32" stroke-width="3"/>
    <path d="M48 70L52 50M62 68L66 48M98 68L94 48M112 70L108 50" stroke="#7a6a4a" stroke-width="2"/>
    <circle cx="52" cy="48" r="3" fill="#8a3030" stroke="none"/>
    <circle cx="66" cy="46" r="3" fill="#8a3030" stroke="none"/>
    <circle cx="94" cy="46" r="3" fill="#8a3030" stroke="none"/>
    <circle cx="108" cy="48" r="3" fill="#8a3030" stroke="none"/>
""",
        "br",
    ),
    "stable": (
        "Ahır",
        """
    <path d="M22 80L80 50L138 80L80 110Z" fill="#c9ae78"/>
    <path d="M22 80V110L80 134V110Z" fill="#9a7a4a"/>
    <path d="M80 110L138 80V110L80 134Z" fill="#b8975e"/>
    <path d="M30 80L80 24L130 80Z" fill="#8b6b3a"/>
    <path d="M30 80L80 24L130 80" fill="none" stroke="#5a4020" stroke-width="2.5"/>
    <path d="M55 134V102H105V134Z" fill="url(#stwood)"/>
    <path d="M70 102V134M90 102V134" stroke="#4e412c" stroke-width="2"/>
    <ellipse cx="118" cy="118" rx="14" ry="10" fill="#6b5535" stroke="none"/>
    <path d="M108 112Q118 100 128 112" fill="#5a4528" stroke="none"/>
    <circle cx="124" cy="108" r="2" fill="#2a2010" stroke="none"/>
    <path d="M100 70V95M110 68V93" stroke="#6a5535" stroke-width="3"/>
""",
        "st",
    ),
    "workshop": (
        "Atölye",
        """
    <path d="M30 82L80 55L130 82L80 109Z" fill="#bba06a"/>
    <path d="M30 82V108L80 130V109Z" fill="url(#wsword)"/>
    <path d="M80 109L130 82V108L80 130Z" fill="#a88c58"/>
    <path d="M38 82L80 30L122 82Z" fill="url(#wsthatch)"/>
    <circle cx="48" cy="118" r="16" fill="none" stroke="#5a4a32" stroke-width="4"/>
    <circle cx="48" cy="118" r="4" fill="#5a4a32" stroke="none"/>
    <path d="M100 95L128 78L132 88L104 105Z" fill="#7a6a50"/>
    <path d="M108 100L118 75" stroke="#4a3a28" stroke-width="3"/>
    <path d="M70 130V105H90V130Z" fill="#4d4533"/>
""",
        "ws",
    ),
    "academy": (
        "Akademi",
        """
    <path d="M55 95L80 80L105 95L80 110Z" fill="#c8b48a"/>
    <path d="M55 95V120L80 134V110Z" fill="url(#acstone)"/>
    <path d="M80 110L105 95V120L80 134Z" fill="#a8a890"/>
    <path d="M58 80L80 40L102 80Z" fill="#8c3026"/>
    <path d="M58 80L80 40L102 80" fill="none" stroke="#5a2018" stroke-width="2"/>
    <path d="M80 40V18" stroke="#5d4d31" stroke-width="2"/>
    <path d="M80 18L98 22L80 26Z" fill="#c9a227"/>
    <rect x="72" y="92" width="10" height="14" fill="#3a4a38" stroke="none"/>
    <rect x="72" y="70" width="10" height="12" fill="#3a4a38" stroke="none"/>
    <path d="M70 134V118H90V134Z" fill="#4d4533"/>
""",
        "ac",
    ),
    "smithy": (
        "Demirci",
        """
    <path d="M28 85L80 58L132 85L80 112Z" fill="#a89068"/>
    <path d="M28 85V110L80 132V112Z" fill="#7a6545"/>
    <path d="M80 112L132 85V110L80 132Z" fill="#968058"/>
    <path d="M36 85L80 35L124 85Z" fill="#5a4a3a"/>
    <path d="M100 50L108 20L116 50" fill="#6a6a6a" stroke="#4a4a4a"/>
    <path d="M104 20Q108 8 112 20" fill="none" stroke="#888" stroke-width="2" opacity=".6"/>
    <ellipse cx="55" cy="118" rx="12" ry="6" fill="#4a4a4a" stroke="none"/>
    <path d="M48 112H62V118H48Z" fill="#3a3a3a" stroke="none"/>
    <path d="M70 132V108H90V132Z" fill="#4d4533"/>
    <circle cx="115" cy="105" r="8" fill="#ff8c40" opacity=".7" stroke="none"/>
    <circle cx="115" cy="105" r="4" fill="#ffcc66" stroke="none"/>
""",
        "sm",
    ),
    "rally_point": (
        "İçtima",
        """
    <ellipse cx="80" cy="100" rx="50" ry="22" fill="#6b7a45" stroke="none"/>
    <ellipse cx="80" cy="100" rx="40" ry="16" fill="#7a8a52" stroke="none"/>
    <path d="M40 70V105" stroke="#5d4d31" stroke-width="3"/>
    <path d="M40 70L62 78L40 86Z" fill="#8c3026"/>
    <path d="M120 68V103" stroke="#5d4d31" stroke-width="3"/>
    <path d="M120 68L142 76L120 84Z" fill="#2e5a8c"/>
    <path d="M80 55V100" stroke="#5d4d31" stroke-width="3"/>
    <path d="M80 55L102 63L80 71Z" fill="#c9a227"/>
    <circle cx="55" cy="108" r="3" fill="#4a3a28" stroke="none"/>
    <circle cx="70" cy="112" r="3" fill="#4a3a28" stroke="none"/>
    <circle cx="90" cy="112" r="3" fill="#4a3a28" stroke="none"/>
    <circle cx="105" cy="108" r="3" fill="#4a3a28" stroke="none"/>
""",
        "rp",
    ),
    "statue": (
        "Heykel",
        """
    <path d="M50 115L80 100L110 115L80 130Z" fill="url(#szstone)"/>
    <path d="M55 115V125L80 135V125Z" fill="#7a7a6a"/>
    <path d="M80 125L105 115V125L80 135Z" fill="#9a9a88"/>
    <rect x="70" y="70" width="20" height="40" fill="#b0b0a0" stroke="none"/>
    <circle cx="80" cy="58" r="12" fill="#c0c0b0" stroke="none"/>
    <path d="M68 78L55 95L62 98L72 85Z" fill="#b0b0a0" stroke="none"/>
    <path d="M92 78L105 95L98 98L88 85Z" fill="#b0b0a0" stroke="none"/>
    <path d="M72 110L68 125H76L78 110Z" fill="#a0a090" stroke="none"/>
    <path d="M88 110L84 125H92L90 110Z" fill="#a0a090" stroke="none"/>
    <path d="M80 46L88 40L80 50Z" fill="#8c3026" stroke="none"/>
""",
        "sz",
    ),
    "market": (
        "Pazar",
        """
    <path d="M20 90L50 70L80 90L50 110Z" fill="#d4a060"/>
    <path d="M20 90V110L50 125V110Z" fill="#a87840"/>
    <path d="M50 110L80 90V110L50 125Z" fill="#c49050"/>
    <path d="M25 90L50 55L75 90Z" fill="#2e6b4a"/>
    <path d="M80 88L110 68L140 88L110 108Z" fill="#d4a060"/>
    <path d="M80 88V108L110 123V108Z" fill="#a87840"/>
    <path d="M110 108L140 88V108L110 123Z" fill="#c49050"/>
    <path d="M85 88L110 53L135 88Z" fill="#8c3026"/>
    <ellipse cx="50" cy="105" rx="6" ry="4" fill="#c9a227" stroke="none"/>
    <ellipse cx="110" cy="103" rx="6" ry="4" fill="#6a8c2e" stroke="none"/>
    <path d="M95 115V100H125V115Z" fill="url(#mkwood)"/>
""",
        "mk",
    ),
    "clay": (
        "Kil ocağı",
        """
    <ellipse cx="80" cy="105" rx="48" ry="20" fill="#8a5a3a" stroke="none"/>
    <ellipse cx="80" cy="100" rx="36" ry="14" fill="#a06a45" stroke="none"/>
    <ellipse cx="80" cy="96" rx="22" ry="8" fill="#6a4028" stroke="none"/>
    <path d="M100 70L115 45L130 70Z" fill="#7a5a40"/>
    <path d="M108 45V30" stroke="#5a4a3a" stroke-width="4"/>
    <path d="M104 28H112" stroke="#5a4a3a" stroke-width="2"/>
    <path d="M45 95L55 75L65 95Z" fill="#b88860" stroke="none"/>
    <path d="M95 98L105 78L115 98Z" fill="#b88860" stroke="none"/>
    <circle cx="50" cy="112" r="5" fill="#c49a70" stroke="none"/>
    <circle cx="110" cy="110" r="6" fill="#c49a70" stroke="none"/>
""",
        "cl",
    ),
    "iron": (
        "Demir madeni",
        """
    <path d="M30 100L80 70L130 100L80 125Z" fill="url(#irstone)"/>
    <path d="M45 100Q80 85 115 100Q80 115 45 100Z" fill="#2a2a28" stroke="none"/>
    <path d="M55 98Q80 90 105 98Q80 108 55 98Z" fill="#1a1a18" stroke="none"/>
    <path d="M100 75L120 40" stroke="#5a4a32" stroke-width="3"/>
    <path d="M116 42L128 38L122 50Z" fill="#6a6a6a"/>
    <path d="M40 70L48 50L56 70" fill="#7a7a6a"/>
    <rect x="35" y="108" width="18" height="8" rx="2" fill="#5a5a50" stroke="none"/>
    <rect x="108" y="106" width="18" height="8" rx="2" fill="#5a5a50" stroke="none"/>
""",
        "ir",
    ),
    "farm": (
        "Çiftlik",
        """
    <path d="M20 95H70V115H20Z" fill="#6a8a3a" stroke="none"/>
    <path d="M20 95L25 90H65L70 95" fill="#7a9a45" stroke="none"/>
    <path d="M25 95V115M35 95V115M45 95V115M55 95V115M65 95V115" stroke="#5a7a30" stroke-width="1"/>
    <path d="M85 75L120 55L145 75L110 95Z" fill="#c4a66a"/>
    <path d="M85 75V100L110 115V95Z" fill="url(#fmwood)"/>
    <path d="M110 95L145 75V100L110 115Z" fill="#b8955f"/>
    <path d="M90 75L120 40L140 75Z" fill="url(#fmthatch)"/>
    <path d="M100 115V95H120V112Z" fill="#4d4533"/>
    <circle cx="55" cy="85" r="8" fill="#d4a020" stroke="none"/>
""",
        "fm",
    ),
    "hiding_place": (
        "Gizli depo",
        """
    <ellipse cx="80" cy="105" rx="45" ry="18" fill="#4a5a35" stroke="none"/>
    <path d="M50 100L80 70L110 100Z" fill="url(#hdwood)"/>
    <path d="M55 98L80 78L105 98Z" fill="#6a5535"/>
    <path d="M70 100L80 88L90 100Z" fill="#3a2a18" stroke="none"/>
    <circle cx="80" cy="95" r="3" fill="#c9a227" stroke="none"/>
    <path d="M60 110L70 105L80 110L90 105L100 110" fill="none" stroke="#5a4a32" stroke-width="2"/>
    <ellipse cx="45" cy="112" rx="8" ry="5" fill="#5a6a40" stroke="none"/>
    <ellipse cx="115" cy="112" rx="8" ry="5" fill="#5a6a40" stroke="none"/>
""",
        "hd",
    ),
    "wall": (
        "Duvar",
        """
    <path d="M15 90L40 75L145 75L145 110L40 110L15 95Z" fill="url(#wlstone)"/>
    <path d="M40 75V55H55V75H70V55H85V75H100V55H115V75H130V55H145V75" fill="#a8a898"/>
    <path d="M15 90L40 75V110L15 95Z" fill="#7a7a6a"/>
    <path d="M60 110V85H90V110Z" fill="#5a5a4a"/>
    <path d="M68 85V110M82 85V110" stroke="#3a3a30" stroke-width="2"/>
    <path d="M50 95H60M95 95H140" stroke="#6a6a5a" stroke-width="1"/>
    <path d="M145 75L155 70V105L145 110Z" fill="#8a8a7a"/>
""",
        "wl",
    ),
}

for kind, (name, body, prefix) in arts.items():
    path = ROOT / f"building-{kind}.svg"
    path.write_text(wrap(name, body, prefix), encoding="utf-8")
    print("wrote", path.name)

print("done", len(arts))
