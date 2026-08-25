*** Settings ***
Documentation       Check that a notification is sent when all instances of an attribute are deleted
...                 and attributeDeleted notification trigger is configured for this specific attribute

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource
Resource            ${EXECDIR}/resources/SubscriptionUtils.resource

Test Setup          Create Initial Subscription And Entity
Test Teardown       Delete Initial Subscription And Entity


*** Variables ***
${subscription_payload_file_path}=      subscriptions/subscription-building-entities-attributeDeleted-specific-attribute-normalized.jsonld
${building_filename}=                   building-multi-instances-attributes.jsonld
${entity_expectation_file_path}=        entity-deleted-name-attribute-instance-normalized.json


*** Test Cases ***
046_22_08 Check That A Notification Is Sent With Matching Entity
    [Documentation]    Delete all instances of an attribute and check the received notification
    [Tags]    sub-notification    5_8_6    4_5_5    since_v1.6.1

    ${response}=    Delete Entity Attributes
    ...    entityId=${entity_id}
    ...    attributeId=name
    ...    datasetId=${EMPTY}
    ...    deleteAll=true
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    204    ${response.status_code}

    ${notification}    ${headers}=    Wait for notification    timeout=${10}

    Should be Equal    ${subscription_id}    ${notification}[subscriptionId]
    Length Should Be    ${notification}[data]    ${1}
    Should be Equal    ${entity_id}    ${notification}[data][0][id]
    Check JSON Value In Response Body
    ...    json_path_expr=['name']['value']
    ...    value_to_check=urn:ngsi-ld:null
    ...    response_body=${notification}[data][0]


*** Keywords ***
Create Initial Subscription And Entity
    Create Subscription And Entity    ${subscription_payload_file_path}    ${building_filename}
    Start Local Server    ${notification_server_host}    ${notification_server_port}

Delete Initial Subscription And Entity
    Delete Subscription    ${subscription_id}
    Delete Entity    ${entity_id}
    Stop Local Server
