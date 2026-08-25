*** Settings ***
Documentation       Check that one can retrieve an entity with format or options query params

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Initial Entity
Test Teardown       Delete Created Entity
Test Template       Retrieve Entity With Format Or Options Query Params


*** Variables ***
${filename}=    building-simple-attributes.jsonld


*** Test Cases ***    FORMAT_VALUE    OPTIONS_VALUE    EXPECTATION_FILENAME
018_15_01 RetrieveWithKeyValuesFormat
    [Documentation]    Check that one can retrieve with format query param having keyValues value
    [Tags]    e-retrieve    5_7_1    6_3_7    since_v1.8.1
    keyValues    ${EMPTY}    building-simple-attributes-simplified.jsonld
018_15_02 RetrieveWithSimplifiedFormat
    [Documentation]    Check that one can retrieve with format query param having simplified value
    [Tags]    e-retrieve    5_7_1    6_3_7    since_v1.8.1
    simplified    ${EMPTY}    building-simple-attributes-simplified.jsonld
018_15_03 RetrieveWithNormalizedFormat
    [Documentation]    Check that one can retrieve with format query param having normalized value
    [Tags]    e-retrieve    5_7_1    6_3_7    since_v1.8.1
    normalized    ${EMPTY}    building-simple-attributes.jsonld
018_15_04 RetrieveWithKeyValuesOptions
    [Documentation]    Check that one can retrieve with options query param having keyValues value
    [Tags]    e-retrieve    5_7_1    6_3_7    since_v1.8.1
    ${EMPTY}    keyValues    building-simple-attributes-simplified.jsonld
018_15_05 RetrieveWithSimplifiedOptions
    [Documentation]    Check that one can retrieve with options query param having simplified value
    [Tags]    e-retrieve    5_7_1    6_3_7    since_v1.8.1
    ${EMPTY}    simplified    building-simple-attributes-simplified.jsonld
018_15_06 RetrieveWithNormalizedOptions
    [Documentation]    Check that one can retrieve with options query param having normalized value
    [Tags]    e-retrieve    5_7_1    6_3_7    since_v1.8.1
    ${EMPTY}    normalized    building-simple-attributes.jsonld
018_15_07 RetrieveWithFormatAndOptions
    [Documentation]    Check that one can retrieve with both format and options query params
    [Tags]    e-retrieve    5_7_1    6_3_7    since_v1.8.1
    keyValues    normalized    building-simple-attributes-simplified.jsonld


*** Keywords ***
Retrieve Entity With Format Or Options Query Params
    [Documentation]    Retrieve an entity giving format or options query params different values
    [Arguments]    ${format_value}    ${options_value}    ${expectation_filename}
    ${response}=    Retrieve Entity
    ...    id=${entity_id}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    ...    format=${format_value}
    ...    options=${options_value}

    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing Entity element
    ...    ${expectation_filename}
    ...    ${entity_id}
    ...    ${response.json()}
    ...    ${True}

Create Initial Entity
    ${entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${entity_id}
    ${create_response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response.status_code}

Delete Created Entity
    Delete Entity    ${entity_id}
