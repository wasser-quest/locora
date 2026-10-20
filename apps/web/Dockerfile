# syntax=docker/dockerfile:1
# The browser app as an unprivileged nginx: static files only. Its data lives in the visitor's browser.
# The build (dist/) comes from `node apps/web/build.mjs`; deploy/build.sh runs it before this image.
FROM nginxinc/nginx-unprivileged:1.27-alpine
COPY nginx.conf /etc/nginx/conf.d/default.conf
COPY dist/ /usr/share/nginx/html/
EXPOSE 8080
