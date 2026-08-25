*** Settings ***
Documentation       Check that one cannot retrieve a Context Source Registration, if the NGSI-LD endpoint does not know about the target context source registration, because there is no existing context source registration whose id (URI) is equivalent

Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Test Cases ***
036_02_01 Retrieve Unknown Context Source Registration
    [Documentation]    Check that one cannot retrieve a Context Source Registration, if the NGSI-LD endpoint does not know about the target context source registration, because there is no existing context source registration whose id (URI) is equivalent
    [Tags]    csr-retrieve    5_10_1
    ${response}=    Retrieve Context Source Registration
    ...    context_source_registration_id=urn:ngsi-ld:ContextSourceRegistration:unknowRegistration
    Check Response Status Code    404    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_RESOURCE_NOT_FOUND}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
