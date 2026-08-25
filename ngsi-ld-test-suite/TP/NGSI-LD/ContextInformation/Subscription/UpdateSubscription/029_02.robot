*** Settings ***
Documentation       Check that one cannot update a subscription: If the NGSI-LD System does not know about the target Subscription, because there is no existing Subscription whose id (URI) is equivalent, an error of type ResourceNotFound shall be raised

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Variables ***
${subscription_update_fragment_file_path}=      subscriptions/fragments/subscription-update.json


*** Test Cases ***
029_02_01 Update Unknown Subscription
    [Documentation]    Check that one cannot update a subscription: If the NGSI-LD System does not know about the target Subscription, because there is no existing Subscription whose id (URI) is equivalent, an error of type ResourceNotFound shall be raised
    [Tags]    sub-update    5_8_2
    ${response}=    Update Subscription
    ...    urn:ngsi-ld:Subscription:unknowSubscription
    ...    ${subscription_update_fragment_file_path}
    ...    ${CONTENT_TYPE_JSON}
    Check Response Status Code    404    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_RESOURCE_NOT_FOUND}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
