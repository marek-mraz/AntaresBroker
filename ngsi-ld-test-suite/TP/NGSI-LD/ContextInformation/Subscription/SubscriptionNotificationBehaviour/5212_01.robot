*** Settings ***
Documentation       Verify that a paused Subscription delivers no notification, and that
...                 resuming it restores delivery.
...
...                 Table 5.2.12-1 defines isActive: "false indicates that the subscription
...                 is paused, and notifications shall not be delivered". The pause is a
...                 delivery rule, not only a presentation one — 5811_01 already asserts
...                 that isActive=false is reported as status "paused", which a broker can
...                 satisfy while still notifying.
...
...                 Antares extension TP. Case 02 is the positive control: the same
...                 subscription, the same entity change, delivery restored by isActive=true.
...                 Without it, case 01 would also pass on a broker that never notifies at
...                 all.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource

Suite Setup         Create A Paused Subscription
Suite Teardown      Remove Subscription And Entity


*** Variables ***
${notification_server_send_url}=    http://${notification_server_host}:${notification_server_port}/notify
${entity_building_filepath}=        building-simple-attributes.jsonld
${fragment_filename}=              airQualityLevel-fragment.jsonld


*** Test Cases ***
5212_01_01 A Paused Subscription Delivers No Notification
    [Documentation]    Table 5.2.12-1 isActive=false: notifications shall not be delivered,
    ...    so a change in a watched Entity must reach no endpoint
    [Tags]    sub-notification    5_2_12    since_v1.9.1

    ${response}=    Create Entity    ${entity_building_filepath}    ${entity_id}
    Check Response Status Code    201    ${response.status_code}
    Wait for no notification    ${5}

5212_01_02 Resuming The Subscription Restores Delivery
    [Documentation]    The same subscription, the same kind of change: with isActive=true
    ...    the notification arrives, so case 01 cannot pass on a silent broker
    [Tags]    sub-notification    5_2_12    since_v1.9.1

    ${resume}=    Evaluate    {"isActive": True}
    ${response}=    Update Subscription With Payload
    ...    ${subscription_id}
    ...    ${resume}
    ...    ${CONTENT_TYPE_JSON}
    Check Response Status Code    204    ${response.status_code}

    ${response}=    Update Entity Attributes
    ...    ${entity_id}
    ...    ${fragment_filename}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    204    ${response.status_code}

    ${notification}    ${headers}=    Wait for notification    ${10}
    Should Be Equal    ${subscription_id}    ${notification}[subscriptionId]
    Should Not Be Empty    ${notification}[data]    Notification data should not be empty
    Should Be Equal    ${entity_id}    ${notification}[data][0][id]


*** Keywords ***
Create A Paused Subscription
    Start Local Server    ${notification_server_host}    ${notification_server_port}
    ${subscription_id}=    Generate Random Subscription Id
    ${entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${subscription_id}
    Set Suite Variable    ${entity_id}
    ${payload}=    Evaluate
    ...    {"id": "${subscription_id}", "type": "Subscription", "entities": [{"type": "Building"}], "isActive": False, "notification": {"endpoint": {"uri": "${notification_server_send_url}"}}, "@context": ["${ngsild_test_suite_context}"]}
    ${response}=    Create Subscription From Subscription Payload    ${payload}
    Check Response Status Code    201    ${response.status_code}
    Sleep    1s

Remove Subscription And Entity
    Delete Subscription    ${subscription_id}
    Delete Entity    ${entity_id}
    Stop Local Server
