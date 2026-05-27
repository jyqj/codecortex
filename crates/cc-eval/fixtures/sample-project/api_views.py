import os
from django.http import JsonResponse
from django.views import View
from app import get_user, create_user


# Environment-based feature toggle
ENABLE_ADMIN = os.environ.get("ENABLE_ADMIN", "false") == "true"
API_VERSION = os.environ.get("API_VERSION", "v1")


def user_detail_view(request, user_id):
    """Return a single user by ID."""
    user = get_user(user_id)
    return JsonResponse({"version": API_VERSION, "user": user})


def user_create_view(request):
    """Create a new user from POST body."""
    name = request.POST.get("name", "anonymous")
    user = create_user(name)
    return JsonResponse({"created": True, "user": user})


def health_view(request):
    """Health-check endpoint for load balancers."""
    return JsonResponse({"status": "ok", "admin_enabled": ENABLE_ADMIN})


class UserListView(View):
    """Class-based view listing all users."""

    def get(self, request):
        users = [get_user(i) for i in range(1, 6)]
        return JsonResponse({"users": users, "count": len(users)})

    def post(self, request):
        name = request.POST.get("name", "")
        if not name:
            return JsonResponse({"error": "name required"}, status=400)
        user = create_user(name)
        return JsonResponse({"created": True, "user": user})


def admin_dashboard_view(request):
    """Admin dashboard — only active when ENABLE_ADMIN is set."""
    if not ENABLE_ADMIN:
        return JsonResponse({"error": "admin disabled"}, status=403)
    return JsonResponse({"admin": True, "version": API_VERSION})


def _internal_format(data):
    """Private helper to normalise response shape."""
    return {"data": data, "ok": True}
