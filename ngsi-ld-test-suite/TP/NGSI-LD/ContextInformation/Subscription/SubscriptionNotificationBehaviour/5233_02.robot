*** Settings ***
Documentation       Check that an EntitySelector whose type is "*" subscribes to every
...                 Entity Type (CIM 009 Table 5.2.33-1: "To indicate a request for all
...                 Entities (with implied local scope), "*" is also allowed as a
...                 value"; clause 5.2.33 scopes EntitySelector to what is "queried or
...                 subscribed to"). A broker that expands "*" as a term instead builds
...                 an IRI no Entity carries: the subscription is created and then
...                 notifies nothing, with no error on any status code.
...
...                 Antares extension TP — the official TPs exercise "*" only on the
...                 query side (5233_01, 019_26), never on a subscription.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource

Suite Setup         Before Test
Suite Teardown      After Test


*** Variables ***
${subscription_payload_file_path}=      subscriptions/subscription-entities-type-star.jsonld
${notification_server_send_url}=        http://${notification_server_host}:${notification_server_port}/notify
${entity_building_filepath}=            building-simple-attributes.jsonld
${content_type}=                        application/ld+json


*** Test Cases ***
5233_02_01 Check That A Star Selector Notifies For Any Entity Type
    [Documentation]    Table 5.2.33-1: a "*" selector type indicates a request for all Entities, so an Entity of any type triggers the notification.
    [Tags]    sub-notification    5_2_33    5_8_6    since_v1.9.1

    ${entity_id}=    Generate Random Building Entity Id
    ${response}=    Create Entity Selecting Content Type
    ...    ${entity_building_filepath}
    ...    ${entity_id}
    ...    ${content_type}
    Set Suite Variable    ${entity_id}

    ${notification}    ${headers}=    Wait for notification    timeout=${5}

    Should be Equal    ${subscription_id}    ${notification}[subscriptionId]
    Dictionary Should Contain Key    ${notification}    data
    Should Not Be Empty    ${notification}[data]    Notification data should not be empty
    Should be Equal    ${entity_id}    ${notification}[data][0][id]


*** Keywords ***
Setup Initial Subscription
    ${subscription_id}=    Generate Random Subscription Id
    ${subscription_payload}=    Load Subscription Sample With Reachable Endpoint
    ...    ${subscription_payload_file_path}
    ...    ${subscription_id}
    ...    ${notification_server_send_url}
    ${create_response}=    Create Subscription From Subscription Payload
    ...    ${subscription_payload}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response.status_code}
    Set Suite Variable    ${subscription_id}

Before Test
    Start Local Server    ${notification_server_host}    ${notification_server_port}
    Sleep    1s
    Setup Initial Subscription

After Test
    Delete Subscription    ${subscription_id}
    Delete Entity    ${entity_id}
    Stop Local Server
