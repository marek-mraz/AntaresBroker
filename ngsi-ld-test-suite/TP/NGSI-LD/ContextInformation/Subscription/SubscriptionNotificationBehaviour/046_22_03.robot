*** Settings ***
Documentation       Check that a notification is not sent when an attribute is deleted
...                 and attributeDeleted notification trigger is not configured for this specific attribute

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource
Resource            ${EXECDIR}/resources/SubscriptionUtils.resource

Test Setup          Create Initial Subscription And Entity
Test Teardown       Delete Initial Subscription And Entity


*** Variables ***
${subscription_payload_file_path}=      subscriptions/subscription-building-entities-attributeDeleted-specific-attribute-keyValues.jsonld
${building_filename}=                   building-simple-attributes.jsonld


*** Test Cases ***
046_22_03 Check That A Notification Is Not Sent With Updated Entity
    [Documentation]    Delete a not watched attribute and check no notification is received
    [Tags]    sub-notification    5_8_6    since_v1.6.1

    ${response}=    Delete Entity Attributes
    ...    entityId=${entity_id}
    ...    attributeId=airQualityLevel
    ...    datasetId=${EMPTY}
    ...    deleteAll=${EMPTY}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    204    ${response.status_code}

    ${notification}    ${headers}=    Wait for no notification    timeout=${10}


*** Keywords ***
Create Initial Subscription And Entity
    Create Subscription And Entity    ${subscription_payload_file_path}    ${building_filename}
    Start Local Server    ${notification_server_host}    ${notification_server_port}

Delete Initial Subscription And Entity
    Delete Subscription    ${subscription_id}
    Delete Entity    ${entity_id}
    Stop Local Server
