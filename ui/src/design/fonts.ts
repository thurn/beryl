// The bundled fonts: the app runs offline. EB Garamond (OFL) for titles and actions, Inter (OFL)
// for instructions, and the Mana font (OFL, github.com/andrewgioia/mana) for mana symbols.
import "@fontsource-variable/eb-garamond/wght.css";
import "@fontsource-variable/eb-garamond/wght-italic.css";
import "@fontsource-variable/inter/wght.css";
import mana from "mana-font/fonts/mana.woff2";

// Registered here rather than by mana-font's stylesheet, which also bundles a proprietary text font.
document.fonts.add(new FontFace("Mana", `url(${mana}) format("woff2")`));
