// The fonts, chosen per role against docs/ui_mockups with `just ui-fonts`: Times New Roman (a macOS
// system font, referenced by name) for titles, actions and captions; Crimson Pro (OFL, bundled:
// the app runs offline) for card names, type lines, rules text and P/T; Open Sans (OFL, bundled)
// for instructions; the Mana font (OFL, github.com/andrewgioia/mana) for mana symbols.
import "@fontsource-variable/crimson-pro/wght.css";
import "@fontsource-variable/open-sans/wght.css";
import mana from "mana-font/fonts/mana.woff2";

// Registered here rather than by mana-font's stylesheet, which also bundles a proprietary text font.
document.fonts.add(new FontFace("Mana", `url(${mana}) format("woff2")`));
