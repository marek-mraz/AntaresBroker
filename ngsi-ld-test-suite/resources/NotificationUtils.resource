*** Settings ***
Documentation       Check Notification Behaviour

Library             BuiltIn
Library             Collections
Library             HttpCtrl.Server
Library             ${EXECDIR}/libraries/assertionUtils.py
Library             ${EXECDIR}/libraries/dateTimeUtils.py
Library             ${EXECDIR}/libraries/logUtils.py
Variables           ${EXECDIR}/resources/variables.py


*** Variables ***
${notification_type}            ContextSourceNotification


*** Keywords ***
Start Local Server
    [Tags]    actor_notification-receiver
    [Arguments]    ${host}=${notification_server_host}    ${port}=${notification_server_port}
    # Initialize HTTP Client And Server
    Start Server    ${host}    ${port}

Wait for notification
    [Tags]    actor_notification-receiver
    [Arguments]    ${timeout}=${15}
    # HTTP server receives it and checks incoming request for correctness
    # .. "Wait For Request"
    # .... This call is blocked until HTTP request arrives or timeout.
    # .... Further detaills: https://annoviko.github.io/robotframework-httpctrl/server.html#Wait%20For%20Request
    Wait For Request    ${timeout}
    Reply By    200

    ${notification_payload}=    Get Request Body
    ${notification_headers}=    Get Request Headers
    ${notification}=    Evaluate    json.loads('''${notification_payload}''')    json
    ${headers}=    Convert To Dictionary    ${notification_headers}
    Output Notification    ${notification}    ${notification_headers}    Wait for notification
    RETURN    ${notification}    ${headers}

Wait for CSource notification and validate it
    [Tags]    actor_notification-receiver
    [Arguments]
    ...    ${expected_subscription_id}
    ...    ${expected_context_source_registration_ids}
    ...    ${expected_trigger_reason}
    ...    ${expected_notification_data_entities}=${EMPTY}
    ...    ${timeout}=${15}
    # HTTP server receives it and checks incoming request for correctness
    # .. "Wait For Request"
    # .... This call is blocked until HTTP request arrives or timeout.
    # .... Further detaills: https://annoviko.github.io/robotframework-httpctrl/server.html#Wait%20For%20Request
    Wait For Request    ${timeout}
    Reply By    200

    ${notification_payload}=    Get Request Body
    ${notification_headers}=    Get Request Headers
    ${notification}=    Evaluate    json.loads('''${notification_payload}''')    json
    ${notification_data_length}=    Get length    ${notification}[data]
    ${expected_notification_data_entities_length}=    Get length    ${expected_notification_data_entities}
    ${expected_notification_data_length}=    Get length    ${expected_context_source_registration_ids}

    Output Notification    ${notification}    ${notification_headers}    Wait for notification
    Should Be Equal    ${notification}[type]    ${notification_type}
    Should Be Equal    ${notification}[subscriptionId]    ${expected_subscription_id}
    # notifiedAt is an RFC 3339 timestamp and may carry fractional seconds
    # (e.g. 2026-05-27T10:28:22.317Z). Validate with the millis-tolerant parser
    # used for createdAt/modifiedAt, not a fixed second-precision format.
    ${notified_at_date}=    Parse Ngsild Date    ${notification}[notifiedAt]
    Should Not Be Equal    ${notified_at_date}    ${None}
    ${index}=    Set Variable    0
    FOR    ${expected_context_source_registration_id}    IN    @{expected_context_source_registration_ids}
        List Should Contain Value
        ...    ${expected_context_source_registration_ids}
        ...    ${notification}[data][${index}][id]
        ${index}=    Evaluate    ${index} + 1
    END
    Should Be Equal    '${notification_data_length}'    '${expected_notification_data_length}'
    Should Be Equal    ${notification}[triggerReason]    ${expected_trigger_reason}

    # TODO Currently we check entities information of the first CSR returned in the notification, find a way to check them all
    IF    ${expected_notification_data_entities_length}>0
        Check Notification Data Entities
        ...    ${notification}[data][0][information]
        ...    ${expected_notification_data_entities}
    END

Wait for no notification
    [Tags]    actor_notification-receiver
    [Arguments]    ${timeout}=${5}
    # HTTP server receives it and checks incoming request for correctness
    # .. "Wait For Request"
    # .... This call is blocked until HTTP request arrives or timeout.
    # .... Further detaills: https://annoviko.github.io/robotframework-httpctrl/server.html#Wait%20For%20Request
    Wait For No Request    ${timeout}

Check Notification Data Entities
    [Arguments]    ${notification_data_information}    ${expected_notification_data_entities}

    ${notification_data_entities}=    Create List
    ${index}=    Set Variable    0
    FOR    ${registration_information}    IN    @{notification_data_information}
        Append To List    ${notification_data_entities}    ${registration_information}[entities][0][type][0]
    END

    Lists Should Be Equal
    ...    ${expected_notification_data_entities}
    ...    ${notification_data_entities}
    ...    ignore_order=True

Stop Local Server
    [Tags]    actor_notification-receiver
    # Terminate HTTP Server
    Stop Server
