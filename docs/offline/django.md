floo - Build a Django app on floo

End-to-end Django 4+ journey: deploy, add Postgres, add per-user auth,
add a custom domain. Every step has runnable Python code in the
published guide.

## 1. Dockerfile

  FROM python:3.12-slim
  ...
  RUN python manage.py collectstatic --noinput
  CMD ["gunicorn", "mysite.wsgi:application", "--bind", "0.0.0.0:8000", "--workers", "3"]

Use whitenoise for static files, gunicorn for the WSGI server.

## 2. settings.py for production

  import dj_database_url

  SECRET_KEY = os.environ["DJANGO_SECRET_KEY"]
  DEBUG = os.environ.get("DJANGO_DEBUG", "false").lower() == "true"
  ALLOWED_HOSTS = [".on.getfloo.com", *os.environ.get("DJANGO_ALLOWED_HOSTS", "").split(",")]

  SECURE_PROXY_SSL_HEADER = ("HTTP_X_FORWARDED_PROTO", "https")
  USE_X_FORWARDED_HOST = True
  SESSION_COOKIE_SECURE = True
  SESSION_COOKIE_HTTPONLY = True
  SESSION_COOKIE_SAMESITE = "Lax"

  DATABASES = {"default": dj_database_url.config(conn_max_age=600)}

## 3. floo init + deploy

  floo init my-django-app

  [services.web]
  type = "web"
  path = "."
  port = 8000
  ingress = "public"
  dev_command = "python manage.py runserver 0.0.0.0:8000"
  migrate_command = "python manage.py migrate --noinput"

  floo preflight
  git add . && git commit -m "chore: configure floo"
  git push origin main
  floo apps github connect owner/my-django-app

  # Set the secret key after first deploy
  python -c 'from django.core.management.utils import get_random_secret_key; print(get_random_secret_key())' | floo env set DJANGO_SECRET_KEY --stdin --secret --app my-django-app
  floo redeploy --app my-django-app

## 4. Postgres

  [managed.default]
  type = "postgres"

  # dj-database-url parses DATABASE_URL automatically

## 5. Per-user auth

  [app]
  access_mode = "accounts"

Add a tiny middleware that reads X-Floo-User-Email / X-Floo-User-Id /
X-Floo-User-Name from request.META (Django prefixes incoming HTTP
headers with HTTP_ and uppercases them):

  class FlooUserMiddleware:
      def __init__(self, get_response): self.get_response = get_response
      def __call__(self, request):
          email = request.META.get("HTTP_X_FLOO_USER_EMAIL")
          request.floo_user = FlooUser(email=email, ...) if email else None
          return self.get_response(request)

## 6. Custom domain

  floo domains add app.example.com --app my-django-app
  floo env set DJANGO_ALLOWED_HOSTS=app.example.com --app my-django-app
  floo redeploy --app my-django-app

## 7. Local dev

  floo dev --app my-django-app

## Gotchas

  - /healthz is reserved by Cloud Run - use /health
  - Bind to 0.0.0.0 in gunicorn
  - DEBUG=False in prod (the default above is False)
  - DJANGO_SECRET_KEY must be set or sessions can be forged
  - SECURE_PROXY_SSL_HEADER required for is_secure() to work behind floo

Full guide with complete Python code: https://getfloo.com/docs/build/django
