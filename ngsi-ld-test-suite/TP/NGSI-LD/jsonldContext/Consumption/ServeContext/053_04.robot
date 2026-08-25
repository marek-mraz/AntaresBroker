*** Settings ***
Documentation       Check that one can get an increase numberOfHits after creation of a Hosted @context and using it

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/jsonldContext.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/HttpUtils.resource

Test Setup          Create Initial @context condition from an external server
Test Teardown       Delete Initial @context condition from an external server


*** Variables ***
${filename}=                @context-minimal-valid.json
${entityfile}=              minimal-entity-using-@context.jsonld
${entity_context_id}=       urn:ngsi-ld:Building:randomUUID


*** Test Cases ***
053_04_01 Check That The numberOfHits Is Increased After Using A Hosted Context
    [Documentation]    Check that the numberOfHits is increased after using a Hosted context
    [Tags]    ctx-serve    5_13_4    since_v1.5.1

    Create Entity selecting @context    ${entityfile}    ${uri}

    ${response}=    Serve a @context
    ...    contextId=${uri}
    ...    details=true

    Check Response Status Code    200    ${response.status_code}
    Check Context Response Body Containing numberOfHits value    ${response.json()}    1


*** Keywords ***
Create Initial @context condition from an external server
    ${response}=    Add a new @context    ${filename}
    Check Response Status Code    201    ${response.status_code}
    ${uri}=    Fetch Id From Response Location Header    ${response.headers}
    Set Suite Variable    ${uri}

    ${response}=    Serve a @context
    ...    contextId=${uri}
    ...    details=true

    Check Response Status Code    200    ${response.status_code}
    Check Context Response Body Containing numberOfHits value    ${response.json()}    0

Delete Initial @context condition from an external server
    Delete Entity    ${entity_context_id}
    Delete a @context    ${uri}
