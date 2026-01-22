# Roxy Website

This directory contains the GitHub Pages website for Roxy.

## Enabling GitHub Pages

1. Go to your repository settings on GitHub
2. Navigate to **Pages** (under Code and automation)
3. Under **Source**, select "Deploy from a branch"
4. Under **Branch**, select `main` (or your default branch) and `/docs` folder
5. Click **Save**

Your site will be available at: `https://roxyproxy.io`

## Custom Domain (Optional)

To use a custom domain:

1. Add a `CNAME` file to this directory with your domain name
2. Configure your DNS provider to point to GitHub Pages
3. Enable "Enforce HTTPS" in repository settings

Example CNAME file content:
```
roxy.yourname.com
```

## Local Development

To preview the site locally, simply open `index.html` in a web browser:

```bash
open docs/index.html
```

Or use a simple HTTP server:

```bash
cd docs
python3 -m http.server 8000
```

Then visit `http://localhost:8000` in your browser.

## Structure

- `index.html` - Main website page
- `style.css` - Styles and responsive design
- `README.md` - This file

## Updating Content

The website is a static HTML page. To update content:

1. Edit `index.html` for content changes
2. Edit `style.css` for styling changes
3. Commit and push to GitHub
4. GitHub Pages will automatically rebuild and deploy

## Notes

- The site is responsive and works on mobile devices
- The GitHub repository is at `ptescher/roxy`
- Consider adding OpenGraph meta tags for better social media sharing
