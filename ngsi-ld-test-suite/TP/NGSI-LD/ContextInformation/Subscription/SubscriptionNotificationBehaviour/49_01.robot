*** Settings ***
Documentation       Verify 4.9 linked-entity subqueries in a Subscription's q
...                 (EXAMPLE 13/14): `attr{path}` resolves the Relationship
...                 target through the broker's own storage when matching
...                 notifications — the Entity whose linked target matches
...                 fires, the one whose target does not match must NOT.
...                 Antares extension TP — the official suite has no linked-q
...                 subscription coverage.

Library             RequestsLibrary
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource

Suite Setup         Start Local Server    ${notification_server_host}    ${notification_server_port}
Suite Teardown      Clean Up


*** Variables ***
${notification_server_send_url}=    http://${notification_server_host}:${notification_server_port}/notify
${sub_id}=                          urn:ngsi-ld:Subscription:linkedq4901
${device_low}=                      urn:ngsi-ld:Device:linkedq-low
${device_full}=                     urn:ngsi-ld:Device:linkedq-full
${vehicle_ok}=                      urn:ngsi-ld:Vehicle:linkedq-ok
${vehicle_low}=                     urn:ngsi-ld:Vehicle:linkedq-low


*** Test Cases ***
49_01_01 Linked Entity Subquery Resolves Through The Broker
    [Documentation]    4.9: q=isConnectedTo{batteryLevel}<0.5 — the Vehicle
    ...    connected to the low-battery Device notifies; the Vehicle
    ...    connected to the healthy Device must NOT (its create precedes the
    ...    matching one, so a wrong match would arrive first).
    [Tags]    sub-notification    4_9    since_v1.9.1

    Create Json Entity    {"id": "${device_low}", "type": "Device", "batteryLevel": {"type": "Property", "value": 0.3}}
    Create Json Entity    {"id": "${device_full}", "type": "Device", "batteryLevel": {"type": "Property", "value": 0.95}}

    &{headers}=    Create Dictionary    Content-Type=application/json
    ${sub}=    Set Variable
    ...    {"id": "${sub_id}", "type": "Subscription", "entities": [{"type": "Vehicle"}], "q": "isConnectedTo{batteryLevel}<0.5", "notification": {"endpoint": {"uri": "${notification_server_send_url}"}}}
    ${response}=    POST    url=${url}/subscriptions    data=${sub}    headers=${headers}    expected_status=any
    Check Response Status Code    201    ${response.status_code}

    Create Json Entity    {"id": "${vehicle_ok}", "type": "Vehicle", "isConnectedTo": {"type": "Relationship", "object": "${device_full}"}}
    Create Json Entity    {"id": "${vehicle_low}", "type": "Vehicle", "isConnectedTo": {"type": "Relationship", "object": "${device_low}"}}

    ${notification}    ${_}=    Wait for notification    timeout=${10}
    Should Be Equal    ${sub_id}    ${notification}[subscriptionId]
    Should Be Equal    ${vehicle_low}    ${notification}[data][0][id]
    Should Not Be Equal    ${vehicle_ok}    ${notification}[data][0][id]


*** Keywords ***
Create Json Entity
    [Arguments]    ${payload}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    POST    url=${url}/entities    data=${payload}    headers=${headers}    expected_status=any
    Check Response Status Code    201    ${response.status_code}

Clean Up
    ${response}=    DELETE    url=${url}/subscriptions/${sub_id}    expected_status=any
    FOR    ${eid}    IN    ${device_low}    ${device_full}    ${vehicle_ok}    ${vehicle_low}
        ${response}=    DELETE    url=${url}/entities/${eid}    expected_status=any
    END
    Stop Local Server
