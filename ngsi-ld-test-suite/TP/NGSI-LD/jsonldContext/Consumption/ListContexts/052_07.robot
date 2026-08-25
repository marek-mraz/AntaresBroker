*** Settings ***
Documentation       Check that one can list all the cached @context entries

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/jsonldContext.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/HttpUtils.resource

Test Setup          Create Initial Cached @context from entity
Test Teardown       Delete Initial @context and entity
Test Template       List @contexts with several previous created @context


*** Variables ***
${entity_filename}=     building-simple-attributes.json
${contextUri}=          https://raw.githubusercontent.com/marek-mraz/ngsi-ld-test-suite/main/resources/jsonld-contexts/ngsi-ld-test-suite.jsonld
${reason_200}=          OK
${reason_204}=          No Content
${content_type}=        application/json


*** Test Cases ***    DETAILS    KIND
052_07_01 List @contexts With Details Set To True And Kind Set To Cached And With Previously Several Add @contexts
    [Tags]    ctx-list    5_13_3    since_v1.5.1
    true    Cached
# 052_05_04 List @contexts with details set to true and kind set to implicitlycreated and with previously several add @contexts
#    [Tags]    ctx-list    5_13_3    since_v1.5.1
#    true    ImplicitlyCreated


*** Keywords ***
Create Initial Cached @context from entity
    ${entity_id}=    Generate Random Building Entity Id
    Set Test Variable    ${entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${entity_filename}
    ...    ${entity_id}
    ...    ${content_type}
    ...    ${contextUri}

    Check Response Status Code    201    ${response.status_code}

List @contexts with several previous created @context
    [Documentation]    Check that one can find a previously cached context
    [Arguments]    ${details}    ${kind}

    ${response}=    List @contexts    ${details}    ${kind}

    Check Response Status Code    200    ${response.status_code}
    Check Response Reason set to    ${response.reason}    ${reason_200}
    ${contextUriList}=    Create List    ${contextUri}
    # One needs to check the list of responses
    Check Context Response Body Containing a list of identifiers
    ...    ${response.json()}
    ...    ${contextUriList}
    ...    ${kind}
    ...    ${TRUE}

Delete Initial @context and entity
    Delete a @context    ${contextUri}
    Delete Entity    ${entity_id}
