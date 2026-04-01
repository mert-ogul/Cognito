import { defineConfig } from "vitepress";

export default defineConfig({
  title: "Cognito",
  description: "Local-first desktop knowledge workspace",
  themeConfig: {
    nav: [
      { text: "Home", link: "/" },
      { text: "Guide", link: "/guide/architecture" },
    ],
    sidebar: [
      {
        text: "Guide",
        items: [
          { text: "Architecture", link: "/guide/architecture" },
          { text: "MVP scope", link: "/guide/mvp-scope" },
          { text: "Privacy", link: "/guide/privacy" },
          { text: "Constitution", link: "/guide/constitution" },
        ],
      },
    ],
  },
});
