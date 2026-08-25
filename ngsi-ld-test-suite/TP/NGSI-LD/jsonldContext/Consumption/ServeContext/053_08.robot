*** Settings ***
Documentation       Check that the numberOfHits is increased after using a ImplicitlyCreated context

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/ApiUtils/jsonldContext.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create initial ImplicitlyCreated @context
Test Teardown       Delete Initial @context Data


*** Variables ***
${subscription_payload_file_path}=      jsonldContext/subscription-with-implicitlycreated-contexts.jsonld
${testing_id_prefix}=                   urn:ngsi-ld:Testing:
${entityfile}=                          minimal-entity-using-@context.jsonld


*** Test Cases ***
053_08_01 Check That The numberOfHits Is Increased After Using A ImplicitlyCreated Context
    [Documentation]    Check that the numberOfHits is increased after using a ImplicitlyCreated context
    [Tags]    ctx-serve    5_13_4    since_v1.5.1

    ${entity_id}=    Generate Random Id    ${testing_id_prefix}
    Set Global Variable    ${entity_id}

    Create Entity selecting @context    ${entityfile}    ${implicit_id_full}    ${entity_id}

    ${response}=    Serve a @context
    ...    contextId=${implicit_id}
    ...    details=true
    ${response}=    Serve a @context
    ...    contextId=${implicit_id}
    ...    details=true

    Check Response Status Code    200    ${response.status_code}
    Check Context Response Body Containing numberOfHits value    ${response.json()}    2


*** Keywords ***
Create initial ImplicitlyCreated @context
    ${subscription_payload}=    Load JSON From File    ${EXECDIR}/data/${subscription_payload_file_path}

    ${subscription_id}=    Get Value From JSON    ${subscription_payload}    $..id
    ${subscription_id}=    Get From List    ${subscription_id}    0

    ${list_contexts}=    Get Value From JSON    ${subscription_payload}    $..@context
    ${list_contexts}=    Get From List    ${list_contexts}    0

    ${response}=    Create Subscription
    ...    ${subscription_id}
    ...    ${subscription_payload_file_path}
    ...    ${CONTENT_TYPE_LD_JSON}

    ${response}=    List @contexts    true    ImplicitlyCreated

    Check Response Status Code    200    ${response.status_code}

    ${response}=    Retrieve Subscription    ${subscription_id}
    ${data}=    Set Variable    ${response.json()}
    ${implicit_id_full}=    Get From Dictionary    ${response.json()}    jsonldContext
    ${implicit_id}=    Evaluate    '${implicit_id_full}'.split('/')[-1]
    Set Global Variable    ${implicit_id}
    Set Global Variable    ${implicit_id_full}
    Set Suite Variable    ${subscription_id}
    Set Suite Variable    ${list_contexts}
    Check Context Response Body Containing numberOfHits value    ${data}    0

Delete Initial @context Data
    Delete Subscription    ${subscription_id}
    FOR    ${uri}    IN    @{list_contexts}
        Log    URI: ${uri}
        Delete a @context    ${uri}
    END
    Delete a @context    ${implicit_id}
    Delete Entity    ${entity_id}
