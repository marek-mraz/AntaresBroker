from __future__ import unicode_literals
from __future__ import division
from json import dumps, JSONDecodeError, loads
from robot.api import logger
from robot.api.deco import keyword


@keyword(name="Output", tags=("I/O",))
def output(response, description, console=True):
    """*Request and response are output to terminal and file (in JSON).*
    :param response: response to a request
    :param description: explains what request is being made
    :param console: If false, the JSON is not written to terminal. Default is true.
    """

    try:
        if response.request.body is None:
            request_body = response.request.body
        else:
            request_body = loads(response.request.body)
    except JSONDecodeError:
        request_body = response.request.body

    try:
        response_body = response.json()
    except JSONDecodeError:
        response_body = None

    request_json = {'method': response.request.method, 'url': response.request.url,
                    'headers': dict(response.request.headers), 'body': request_body}
    response_json = {'url': response.url, 'headers': dict(response.headers), 'status_code': response.status_code,
                     'reason': response.reason, 'body': response_body}

    pretty_request_json = dumps(request_json, indent=4, sort_keys=False, separators=(",", ": "))
    pretty_response_json = dumps(response_json, indent=4, sort_keys=False, separators=(",", ": "))

    logger.info("\n" + description, also_console=True)
    logger.info("Request ->", also_console=True)
    logger.info(pretty_request_json, also_console=True)
    logger.info("Response ->", also_console=True)
    logger.info(pretty_response_json, also_console=True)


@keyword(name="Output Notification", tags=("I/O",))
def output_notification(body, headers, description, console=True):
    """*Body and headers of the notification are output to terminal and file (in JSON).*
    :param body: body of the notification received
    :param headers: headers of the notification received
    :param description: explains what request is being made
    :param console: If false, the JSON is not written to terminal. Default is true.
    """

    request_json = {'headers': dict(headers), 'body': body}

    pretty_request_json = dumps(request_json, indent=4, sort_keys=False, separators=(",", ": "))

    logger.info("\n" + description, also_console=True)
    logger.info("Notification ->", also_console=True)
    logger.info(pretty_request_json, also_console=True)
