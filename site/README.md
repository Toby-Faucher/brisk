# Brisk homepage

The static page for brisk.tfaucher.com. Plain HTML, CSS and a little JavaScript: no build step.

- `index.html` is the page, `style.css` and `app.js` are its styles and script, `404.html` is the not found page.
- `img/` holds the screenshots and the logo. The screenshots are the real app running on demo data (a made up host and generated covers), so no real library, hostname or account shows.
- Icons are inline [Phosphor](https://phosphoricons.com) icons (MIT).
- Type is Geist and Geist Mono from Google Fonts. Colors and radii follow the app's tokens in `web/src/styles/tokens.css`.

## Preview

```sh
python3 -m http.server 4458 --directory site
```

Then open http://127.0.0.1:4458/.

## Not done yet

- Hosting. Nothing here deploys the page, and there is no `CNAME` file.
- The install command on the page points at `https://brisk.tfaucher.com/install.sh`, which does not exist yet.
- The Stream section has a real capture of a stream to add. The page has no stream screenshot yet.
