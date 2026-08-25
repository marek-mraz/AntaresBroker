*** Settings ***
Documentation       Check that the Query Language ValueList, Range and notPatternOp
...                 constructs are supported (CIM 009 clause 4.9).
...
...                 All three are normative grammar (ABNF, p.85): ValueList = Value 1*(, Value);
...                 Range = ComparableValue..ComparableValue; notPatternOp = !~=. Semantics:
...                 Equal p.90 (list: "identical or equivalent to any of the list values";
...                 range: "in the interval between the minimum and maximum of the range (both
...                 included)"), Unequal p.91 (range: "not in the interval ... (e.g. matches 9)").
...                 Lists and ranges pair ONLY with ==/!= (p.84: CompEqualityValue) — an
...                 ordering operator over a Range violates the grammar and shall be rejected.
...
...                 Antares extension TP — the suite exercises none of these constructs.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Entities
Suite Teardown      Delete Initial Entities
Test Template       Query Entities Expecting Count


*** Variables ***
${first_entity_filename}=       building-simple-attributes.jsonld
${second_entity_filename}=      building-simple-attributes-second.jsonld
${entity_type}=                 Building


*** Test Cases ***    Q_PARAMETER    EXPECTED_STATUS    EXPECTED_COUNT
019_28_01 Range With Equal Matches The Closed Interval
    [Documentation]    4.9 p.90: airQualityLevel values are 4 and 6; ==4..5 includes the
    ...    minimum, so exactly the value 4 matches
    [Tags]    e-query    4_9    since_v1.9.1
    airQualityLevel==4..5    200    1

019_28_02 Range With Unequal Matches Outside The Interval
    [Documentation]    4.9 p.91: "not in the interval between the minimum and the maximum
    ...    (both included)" — only the value 6 is outside 4..5
    [Tags]    e-query    4_9    since_v1.9.1
    airQualityLevel!=4..5    200    1

019_28_03 ValueList With Equal Matches Any Listed Value
    [Documentation]    4.9 p.90: "identical or equivalent to any of the list values" —
    ...    both fixture names are listed, so both entities match
    [Tags]    e-query    4_9    since_v1.9.1
    name=="Eiffel Tower","Pisa Tower"    200    2

019_28_04 ValueList With Unequal Excludes Every Listed Value
    [Documentation]    4.9 p.91: "neither identical nor equivalent to any of the list
    ...    values" — only "Pisa Tower" differs from both listed values
    [Tags]    e-query    4_9    since_v1.9.1
    name!="Eiffel Tower","Big Ben"    200    1

019_28_05 Do Not Match Pattern Operator
    [Documentation]    4.9 p.92 notPatternOp: "the target value shall not be in the L(R)
    ...    of the regular pattern" — only "Pisa Tower" escapes ^Eiffel
    [Tags]    e-query    4_9    since_v1.9.1
    name!~="^Eiffel"    200    1

019_28_06 Range With An Ordering Operator Is Rejected
    [Documentation]    4.9 p.84: ordering operators take a single ComparableValue; a Range
    ...    is reachable only through ==/!= (CompEqualityValue) — grammar violation, 400
    [Tags]    e-query    4_9    since_v1.9.1
    airQualityLevel>4..5    400    ${None}


*** Keywords ***
Query Entities Expecting Count
    [Arguments]    ${q}    ${expected_status_code}    ${expected_count}
    ${response}=    Query Entities
    ...    entity_types=${entity_type}
    ...    q=${q}
    ...    count=true
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    ${expected_status_code}    ${response.status_code}
    IF    $expected_count is not None
        Check Response Headers Containing NGSILD-Results-Count Equals To
        ...    ${expected_count}
        ...    ${response.headers}
    END

Setup Initial Entities
    ${first_entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${first_entity_id}
    ${create_response1}=    Create Entity Selecting Content Type
    ...    ${first_entity_filename}
    ...    ${first_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response1.status_code}
    ${second_entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${second_entity_id}
    ${create_response2}=    Create Entity Selecting Content Type
    ...    ${second_entity_filename}
    ...    ${second_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response2.status_code}

Delete Initial Entities
    Delete Entity    ${first_entity_id}
    Delete Entity    ${second_entity_id}
