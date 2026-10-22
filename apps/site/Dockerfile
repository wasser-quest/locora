# syntax=docker/dockerfile:1
# The static site as an unprivileged nginx: it runs as a non-root user and listens on 8080.
FROM nginxinc/nginx-unprivileged:1.27-alpine
COPY nginx.conf /etc/nginx/conf.d/default.conf
COPY index.html privacy.html impressum.html konto-loeschen.html style.css /usr/share/nginx/html/
COPY img/ /usr/share/nginx/html/img/
EXPOSE 8080
