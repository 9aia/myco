import { defineConfig } from "vitepress";
import llmstxt from "vitepress-plugin-llms";

export default defineConfig({
  vite: {
    plugins: [llmstxt()],
  },
  title: "myco",
  description: "project management, grown not built — agent-first PM",
  base: "/myco/",
  ignoreDeadLinks: [/\/api\//],
  themeConfig: {
    nav: [
      { text: "Getting Started", link: "/getting-started" },
      { text: "Tutorial", link: "/tutorials/first-item" },
      { text: "How-to", link: "/how-to/handoff" },
      { text: "Explanation", link: "/explanation/architecture" },
      { text: "Reference", link: "/reference/cli" },
      {
        text: "LLMs",
        items: [
          { text: "llms.txt", link: "https://9aia.com/myco/llms.txt", target: "_blank" },
          { text: "llms-full.txt", link: "https://9aia.com/myco/llms-full.txt", target: "_blank" },
        ],
      },
    ],
    sidebar: [
      {
        text: "Tutorials",
        items: [{ text: "Your first item", link: "/tutorials/first-item" }],
      },
      {
        text: "How-to",
        items: [
          { text: "Hand off work between sessions", link: "/how-to/handoff" },
          { text: "Org-wide views", link: "/how-to/org-views" },
        ],
      },
      {
        text: "Reference",
        items: [
          { text: "CLI", link: "/reference/cli" },
          { text: "Config", link: "/reference/config" },
          { text: "Crate API", link: "/reference/crate-api" },
        ],
      },
      {
        text: "Explanation",
        items: [
          { text: "Architecture", link: "/explanation/architecture" },
          { text: "Release model", link: "/explanation/release-model" },
        ],
      },
    ],
    socialLinks: [{ icon: "github", link: "https://github.com/9aia/myco" }],
  },
});
