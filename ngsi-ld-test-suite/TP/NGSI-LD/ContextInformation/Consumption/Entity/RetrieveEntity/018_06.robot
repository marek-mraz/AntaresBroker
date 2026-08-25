*** Settings ***
Documentation       Check that the JSON-LD @context is obtained from a Link header if present and that the default JSON-LD @context is used if not present

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Entity
Test Teardown       Delete Created Entity
Test Template       Review JSON-LD Resolution When retrieving an entity


*** Variables ***
${filename}=                                building-simple-attributes.json
${empty_jsonld_expectation_filename}=       building-simple-attributes-expanded.json
${creation_jsonld_expectation_filename}=    building-simple-attributes-compacted.json


*** Test Cases ***    CONTEXT    EXPECTED_PAYLOAD
018_06_01 EmptyJsonLdContext
    [Tags]    e-retrieve    6_3_5
    ${EMPTY}    ${empty_jsonld_expectation_filename}
018_06_02 CreationTimeJsonLdContext
    [Tags]    e-retrieve    6_3_5
    ${ngsild_test_suite_context}    ${creation_jsonld_expectation_filename}


*** Keywords ***
Review JSON-LD Resolution When retrieving an entity
    [Documentation]    Check that the JSON-LD @context is obtained from a Link header if present and that the default JSON-LD @context is used if not present
    [Arguments]    ${context}    ${expected_payload}
    ${response}=    Retrieve Entity
    ...    id=${entity_id}
    ...    accept=${CONTENT_TYPE_JSON}
    ...    context=${context}
    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing Entity element    ${expected_payload}    ${entity_id}    ${response.json()}

Setup Initial Entity
    ${entity_id}=    Generate Random Building Entity Id
    ${create_response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_JSON}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    201    ${create_response.status_code}
    Set Test Variable    ${entity_id}

Delete Created Entity
    Delete Entity    ${entity_id}
