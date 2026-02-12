"""Structured error types for the Floo CLI."""


class FlooError(Exception):
    """Base error for all Floo CLI errors."""

    def __init__(self, code: str, message: str, suggestion: str | None = None) -> None:
        self.code = code
        self.message = message
        self.suggestion = suggestion
        super().__init__(message)


class FlooAPIError(FlooError):
    """Error returned from the Floo API."""

    def __init__(self, status_code: int, code: str, message: str) -> None:
        self.status_code = status_code
        super().__init__(code=code, message=message)
